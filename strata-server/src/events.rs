//! Events: what happened to the cluster and who did what, in order, for
//! the command center's Timeline and for webhooks.
//!
//! A server keeps the latest [`RING`] events in memory and, when it has a
//! log store, writes each to its table `events` (kept [`RETAIN_DAYS`]
//! days), which `GET /v1/events` reads for ranges older than memory.
//!
//! Events come from three places:
//!
//! - a coordinator's cluster snapshots ([`Events::observe`]): each new one
//!   is compared with the one before, which finds a copy that went down or
//!   came back (`copy_down`, `copy_up`), a new primary (`primary_changed`),
//!   a shard that lost or regained its majority (`majority_lost`,
//!   `majority_back`), and shards and copies added or removed;
//! - the audit log: every change audited is a `change` event too, and a
//!   note (`POST /v1/events/notes`) is a `note`;
//! - other code, through [`Events::add`] (alerts, rollouts, log levels).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value as Json};

/// Events kept in memory.
pub const RING: usize = 20_000;
/// Days the log store keeps events.
pub const RETAIN_DAYS: u64 = 30;

/// What happened: `kind` (`copy_down`, `primary_changed`, `change`,
/// `note`, ...), `severity` (`info`, `warning` or `critical`), a `title`
/// of one sentence, its `detail` (JSON, or null), where it happened
/// (`where_`: `{"shard"?, "copy"?, "table"?, "rule"?}`, or null) and who
/// did it (a key's name), when someone did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Event {
    pub kind: String,
    pub severity: String,
    pub title: String,
    #[serde(skip_serializing_if = "Json::is_null")]
    pub detail: Json,
    #[serde(rename = "where", skip_serializing_if = "Json::is_null")]
    pub where_: Json,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
}

impl Event {
    /// An event of `kind` with its `severity` and `title`, nothing else.
    pub fn new(kind: impl Into<String>, severity: impl Into<String>, title: impl Into<String>) -> Event {
        Event { kind: kind.into(), severity: severity.into(), title: title.into(), ..Default::default() }
    }
}

/// An event kept: its id (increasing; a time in microseconds unless two
/// came in one), when it happened, and until when it held, if it ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Recorded {
    pub id: i64,
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: Event,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<u64>,
}

/// The events of a server.
pub struct Events {
    ring: Mutex<VecDeque<Arc<Recorded>>>,
    /// When this server started keeping events.
    started_ms: u64,
    last_id: AtomicI64,
    /// Where each event goes besides memory (the log store).
    sink: OnceLock<Box<dyn Fn(&Recorded) + Send + Sync>>,
    /// What snapshots told before: each shard's latest primary.
    seen: Mutex<Seen>,
}

#[derive(Default)]
struct Seen {
    /// By shard: the primary's url and short name, and its term.
    primary: HashMap<u64, (String, String, u64)>,
}

impl Default for Events {
    fn default() -> Events {
        Events { ring: Mutex::new(VecDeque::new()), started_ms: crate::telemetry::now_ms(), last_id: AtomicI64::new(0), sink: OnceLock::new(), seen: Mutex::new(Seen::default()) }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Events {
    pub fn new() -> Events {
        Events::default()
    }

    /// Writes each event added from now on to `f` too (the log store).
    pub fn sink(&self, f: impl Fn(&Recorded) + Send + Sync + 'static) {
        let _ = self.sink.set(Box::new(f));
    }

    /// Keeps `e`, as of now. Returns it as kept.
    pub fn add(&self, e: Event) -> Arc<Recorded> {
        self.add_at(e, crate::telemetry::now_ms(), None)
    }

    /// Keeps `e` as having happened at `ts_ms` (a note about an earlier
    /// moment), held until `until_ms` if it ended.
    pub fn add_at(&self, e: Event, ts_ms: u64, until_ms: Option<u64>) -> Arc<Recorded> {
        let now_us = (crate::telemetry::now_ms() * 1000) as i64;
        let id = loop {
            let last = self.last_id.load(Ordering::Relaxed);
            let id = now_us.max(last + 1);
            if self.last_id.compare_exchange(last, id, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                break id;
            }
        };
        let r = Arc::new(Recorded { id, ts_ms, event: e, until_ms });
        {
            let mut ring = lock(&self.ring);
            if ring.len() >= RING {
                ring.pop_front();
            }
            // In time order: a note about an earlier moment goes before
            // what came after it.
            let at = ring.iter().rposition(|x| x.ts_ms <= ts_ms).map_or(0, |i| i + 1);
            ring.insert(at, r.clone());
        }
        if let Some(f) = self.sink.get() {
            f(&r);
        }
        r
    }

    /// The events kept in memory from `from_ms` to `to_ms` (inclusive) of
    /// the `kinds` given (all when empty), the newest first, `limit` at
    /// most; and the time of the oldest event memory holds.
    pub fn range(&self, from_ms: u64, to_ms: u64, kinds: &[String], limit: usize) -> (Vec<Arc<Recorded>>, Option<u64>) {
        let ring = lock(&self.ring);
        let oldest = ring.front().map(|r| r.ts_ms);
        let v = ring
            .iter()
            .rev()
            .filter(|r| r.ts_ms >= from_ms && r.ts_ms <= to_ms && (kinds.is_empty() || kinds.iter().any(|k| *k == r.event.kind)))
            .take(limit)
            .cloned()
            .collect();
        (v, oldest)
    }

    /// From when memory holds every event: since the server started, or
    /// since its oldest once it holds [`RING`]. What came before is in the
    /// log store.
    pub fn held_from(&self) -> u64 {
        let ring = lock(&self.ring);
        match ring.front() {
            Some(r) if ring.len() >= RING => r.ts_ms,
            _ => self.started_ms,
        }
    }

    /// Adds the events that tell `before` from `after`, two cluster
    /// snapshots (`/v1/cluster`) one after the other. The first snapshot
    /// (`before` none) only sets what the next is compared with.
    pub fn observe(&self, before: Option<&Json>, after: &Json) {
        for e in self.diff(before, after) {
            self.add(e);
        }
    }

    /// The events of [`Self::observe`].
    pub fn diff(&self, before: Option<&Json>, after: &Json) -> Vec<Event> {
        let mut out = Vec::new();
        let shards = |j: &Json| j["shards"].as_array().cloned().unwrap_or_default();
        let after_shards = shards(after);
        let mut seen = lock(&self.seen);
        let Some(before) = before else {
            for s in &after_shards {
                if let Some(p) = primary_of(s) {
                    seen.primary.insert(s["shard"].as_u64().unwrap_or(0), p);
                }
            }
            return out;
        };
        let before_shards = shards(before);
        let n = |s: &Json| s["shard"].as_u64().unwrap_or(0);
        let copies = |s: &Json| s["replicas"].as_array().cloned().unwrap_or_default();
        let place = |shard: u64, r: &Json| {
            let mut j = json!({"shard": shard, "copy": short(r)});
            if let Some(name) = name_of(r) {
                j["name"] = json!(name);
            }
            j
        };
        for s in &after_shards {
            let i = n(s);
            let Some(b) = before_shards.iter().find(|b| n(b) == i) else {
                let urls: Vec<String> = copies(s).iter().map(short).collect();
                out.push(Event { detail: json!({"copies": urls}), where_: json!({"shard": i}), ..Event::new("shard_added", "info", format!("Shard {i} was added, with {} copies", urls.len())) });
                continue;
            };
            let (was, now) = (copies(b), copies(s));
            for r in &now {
                let url = &r["url"];
                match was.iter().find(|w| w["url"] == *url) {
                    None => out.push(Event { detail: json!({"url": url}), where_: place(i, r), ..Event::new("copy_added", "info", format!("{} was added to shard {i}", label(r))) }),
                    Some(w) => {
                        let (down_before, down_now) = (w["status"] == "down", r["status"] == "down");
                        if !down_before && down_now {
                            let role = r["last_role"].as_str().or(w["role"].as_str()).unwrap_or("copy");
                            let mut detail = json!({"url": url, "last_role": role});
                            if let Some(e) = r["error"].as_str() {
                                detail["error"] = json!(e);
                            }
                            out.push(Event { detail, where_: place(i, r), ..Event::new("copy_down", "warning", format!("{} ({}{role} of shard {i}) does not answer", short(r), name_of(r).map_or(String::new(), |n| format!("{n}, ")))) });
                        } else if down_before && !down_now {
                            let since = w["down_since_ms"].as_u64();
                            let mut detail = json!({"url": url, "role": r["role"]});
                            let mut title = format!("{} is back", label(r));
                            if let Some(t) = since {
                                let s = crate::telemetry::now_ms().saturating_sub(t) / 1000;
                                detail["down_s"] = json!(s);
                                title = format!("{} is back, after {s} s down", label(r));
                            }
                            out.push(Event { detail, where_: place(i, r), ..Event::new("copy_up", "info", title) });
                        }
                    }
                }
            }
            for w in &was {
                if !now.iter().any(|r| r["url"] == w["url"]) {
                    out.push(Event { detail: json!({"url": w["url"]}), where_: place(i, w), ..Event::new("copy_removed", "info", format!("{} was removed from shard {i}", label(w))) });
                }
            }
            if s["failover"] == true || b["failover"] == true {
                match (b["majority"].as_bool(), s["majority"].as_bool()) {
                    (Some(true), Some(false)) => out.push(Event {
                        detail: json!({"why": s["why"]}),
                        where_: json!({"shard": i}),
                        ..Event::new("majority_lost", "critical", format!("Shard {i} lost its majority"))
                    }),
                    (Some(false), Some(true)) => out.push(Event { detail: json!({"why": s["why"]}), where_: json!({"shard": i}), ..Event::new("majority_back", "info", format!("Shard {i} has a majority again")) }),
                    _ => {}
                }
            }
            if let Some(p) = primary_of(s) {
                if let Some(old) = seen.primary.get(&i).filter(|o| o.0 != p.0) {
                    // A failover when the old primary no longer answers; a
                    // handover by hand when it still does.
                    let old_down = now.iter().find(|r| r["url"] == json!(old.0)).is_none_or(|r| r["status"] == "down");
                    out.push(Event {
                        detail: json!({"from": old.1, "to": p.1, "from_url": old.0, "to_url": p.0, "term": p.2, "automatic": old_down}),
                        where_: json!({"shard": i, "copy": p.1}),
                        ..Event::new(
                            "primary_changed",
                            if old_down { "warning" } else { "info" },
                            format!("{} is shard {i}'s primary in place of {}{}", p.1, old.1, if old_down { " (failover)" } else { "" }),
                        )
                    });
                }
                seen.primary.insert(i, p);
            }
        }
        for b in &before_shards {
            let i = n(b);
            if !after_shards.iter().any(|s| n(s) == i) {
                out.push(Event { where_: json!({"shard": i}), ..Event::new("shard_removed", "info", format!("Shard {i} was removed")) });
                seen.primary.remove(&i);
            }
        }
        out
    }
}

/// A copy's short name (`s1-0`), else its url.
fn short(r: &Json) -> String {
    r["short"].as_str().or(r["url"].as_str()).unwrap_or("?").to_string()
}

/// A copy's server's name, when it said one other than its short name (a
/// copy down keeps the one it said last).
fn name_of(r: &Json) -> Option<&str> {
    r["name"].as_str().filter(|n| !n.is_empty() && Some(*n) != r["short"].as_str())
}

/// A copy in a title: `s0-2 (spare-4)`, or `s0-2` without a name.
fn label(r: &Json) -> String {
    match name_of(r) {
        Some(n) => format!("{} ({n})", short(r)),
        None => short(r),
    }
}

/// A shard's primary in a snapshot: its url, short name and term.
fn primary_of(s: &Json) -> Option<(String, String, u64)> {
    let url = s["primary"].as_str()?;
    let r = s["replicas"].as_array()?.iter().find(|r| r["url"] == url)?;
    Some((url.to_string(), short(r), r["term"].as_u64().unwrap_or(0)))
}

/// The event of an audited change (`kind` `change`): who did what.
pub fn of_change(e: &crate::keys::Entry) -> Event {
    let what = e.detail.clone().unwrap_or_else(|| format!("{} {}", e.method, e.route));
    // cluster2: a failed change says why, as its answer did (AC-3).
    let error = e.after.as_ref().and_then(|a| a["error"].as_str()).filter(|_| e.status >= 400);
    let title = match error {
        Some(why) => format!("{}: {what} (it failed: {why})", e.who),
        None if e.status >= 400 => format!("{}: {what} (it failed: {})", e.who, e.status),
        None => format!("{}: {what}", e.who),
    };
    let mut detail = json!({"method": e.method, "route": e.route, "status": e.status, "actor": e.actor});
    if let Some(why) = error {
        detail["error"] = json!(why);
    }
    for (k, v) in [("object", &e.object), ("reason", &e.reason), ("request_id", &e.request_id), ("client", &e.client)] {
        if let Some(v) = v {
            detail[k] = json!(v);
        }
    }
    let where_ = e.object.as_ref().map_or(Json::Null, |o| match o.strip_prefix("shard ").and_then(|n| n.parse::<u64>().ok()) {
        Some(n) => json!({"shard": n}),
        None => match o.strip_prefix("table ") {
            Some(t) => json!({"table": t}),
            None => Json::Null,
        },
    });
    let severity = if e.status >= 400 { "warning" } else { "info" };
    Event { kind: "change".into(), severity: severity.into(), title, detail, where_, who: Some(e.who.clone()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shard(i: u64, primary: &str, copies: &[(&str, bool)], majority: bool) -> Json {
        let replicas: Vec<Json> = copies
            .iter()
            .enumerate()
            .map(|(k, (url, up))| json!({"url": url, "short": format!("s{i}-{k}"), "status": if *up { "ok" } else { "down" }, "role": if *url == primary && *up { "primary" } else if *up { "replica" } else { "unknown" }, "term": 2}))
            .collect();
        json!({"shard": i, "primary": if primary.is_empty() { Json::Null } else { json!(primary) }, "failover": true, "majority": majority, "why": "x", "replicas": replicas})
    }

    fn kinds(v: &[Event]) -> Vec<&str> {
        v.iter().map(|e| e.kind.as_str()).collect()
    }

    #[test]
    fn a_failover_is_a_copy_down_then_a_new_primary() {
        let ev = Events::new();
        let a = json!({"shards": [shard(0, "a", &[("a", true), ("b", true), ("c", true)], true)]});
        assert!(ev.diff(None, &a).is_empty());
        let b = json!({"shards": [shard(0, "", &[("a", false), ("b", true), ("c", true)], true)]});
        let d = ev.diff(Some(&a), &b);
        assert_eq!(kinds(&d), ["copy_down"]);
        assert_eq!(d[0].title, "s0-0 (primary of shard 0) does not answer");
        // With its server's name, which a copy down keeps.
        let named = |j: &Json| {
            let mut j = j.clone();
            j["shards"][0]["replicas"][0]["name"] = json!("spare-4");
            j
        };
        let d = Events::new().diff(Some(&named(&a)), &named(&b));
        assert_eq!((d[0].title.as_str(), &d[0].where_["name"]), ("s0-0 (spare-4, primary of shard 0) does not answer", &json!("spare-4")));
        let d = Events::new().diff(Some(&named(&b)), &named(&a));
        assert_eq!(d.iter().find(|e| e.kind == "copy_up").map(|e| e.title.as_str()), Some("s0-0 (spare-4) is back"));
        let c = json!({"shards": [shard(0, "b", &[("a", false), ("b", true), ("c", true)], true)]});
        let d = ev.diff(Some(&b), &c);
        assert_eq!(kinds(&d), ["primary_changed"]);
        assert_eq!(d[0].detail["from"], "s0-0");
        assert_eq!(d[0].detail["to"], "s0-1");
        assert_eq!(d[0].detail["automatic"], true);
        assert_eq!(d[0].severity, "warning");
    }

    #[test]
    fn a_majority_lost_and_back_and_copies_added() {
        let ev = Events::new();
        let a = json!({"shards": [shard(0, "a", &[("a", true), ("b", true)], true)]});
        ev.diff(None, &a);
        let b = json!({"shards": [shard(0, "a", &[("a", true), ("b", false)], false)]});
        assert_eq!(kinds(&ev.diff(Some(&a), &b)), ["copy_down", "majority_lost"]);
        let c = json!({"shards": [shard(0, "a", &[("a", true), ("b", true), ("c", true)], true)], });
        assert_eq!(kinds(&ev.diff(Some(&b), &c)), ["copy_up", "copy_added", "majority_back"]);
        let d = json!({"shards": [shard(0, "a", &[("a", true), ("c", true)], true), shard(1, "x", &[("x", true)], true)]});
        assert_eq!(kinds(&ev.diff(Some(&c), &d)), ["copy_removed", "shard_added"]);
    }

    #[test]
    fn memory_keeps_events_in_time_order_and_answers_ranges() {
        let ev = Events::new();
        let a = ev.add_at(Event::new("note", "info", "first"), 1000, None);
        let b = ev.add_at(Event::new("change", "info", "second"), 3000, None);
        let c = ev.add_at(Event::new("note", "info", "between"), 2000, None);
        assert!(a.id < b.id && b.id < c.id);
        let (all, oldest) = ev.range(0, u64::MAX, &[], 10);
        assert_eq!(all.iter().map(|r| r.event.title.as_str()).collect::<Vec<_>>(), ["second", "between", "first"]);
        assert_eq!(oldest, Some(1000));
        let (notes, _) = ev.range(1500, 5000, &["note".into()], 10);
        assert_eq!(notes.len(), 1);
        let j = serde_json::to_value(&*notes[0]).unwrap();
        assert_eq!(j["kind"], "note");
        assert!(j.get("where").is_none() && j.get("until_ms").is_none(), "{j}");
    }

    #[test]
    fn a_failed_change_says_why_it_failed() {
        let mut e: crate::keys::Entry = serde_json::from_value(json!({"ts_ms": 1, "who": "root token", "method": "POST", "route": "/v1/admin/shards/:i/replicas", "status": 503, "ms": 1.0,
            "detail": "add http://h:1 to shard 1", "object": "shard 1", "after": {"error": "http://h:1 does not answer"}})).unwrap();
        let ev = of_change(&e);
        assert_eq!(ev.title, "root token: add http://h:1 to shard 1 (it failed: http://h:1 does not answer)");
        assert_eq!((ev.severity.as_str(), &ev.detail["error"], &ev.where_), ("warning", &json!("http://h:1 does not answer"), &json!({"shard": 1})));
        // One that answered: no error, said as done.
        e.status = 200;
        e.detail = Some("added http://h:1 to shard 1".into());
        let ev = of_change(&e);
        assert_eq!((ev.title.as_str(), ev.severity.as_str(), ev.detail.get("error")), ("root token: added http://h:1 to shard 1", "info", None));
    }
}
