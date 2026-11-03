//! Alert rules over the series ([`crate::telemetry::Series`]) and, on a
//! coordinator, over the cluster's state.
//!
//! A rule names a value of each point (`p99_ms`, `error_ratio`,
//! `disk_used_ratio`, `replica_behind_max` ...), how the points of a
//! window make one value (`eval`: their `mean`, `max`, `min`, the `rate`
//! of a count, or `all`, the value that every point must pass), a
//! comparison and a threshold, how long the condition must hold before the
//! alert fires (`for_s`), and the threshold that resolves it (`resolve`,
//! by default the threshold itself). A window without any value of the
//! metric is no data: it keeps the alert as it was (a firing alert stays
//! firing) and never resolves it.
//!
//! A rule's `scope` is `self` (this server's points) or `each_copy`: every
//! copy of every shard evaluates it on its own points and reports its
//! states in its detail (`detail.alerts`, which `/v1/cluster` carries), and
//! a coordinator gathers them into one state per rule with `where`, the
//! copies that are not ok. A coordinator also has built-in rules over the
//! cluster's state ([`builtins`]): a shard without a majority, a copy down,
//! a shard under-replicated, a primary that changed, partial answers, a
//! copy that stopped reporting, and the availability of searches.
//!
//! Each change of an alert (firing, resolved, acknowledged, silenced, a
//! note) is an event, kept in memory, in the log store's `alerts` table
//! for 7 days, and in the server's events; firing and resolved ones are
//! posted to the routes ([`Route`]) that take their severity, unless the
//! alert is silenced. Rules and routes are kept in `--alerts FILE` (JSON),
//! or in memory.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::telemetry::now_ms;
use crate::ApiError;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// The point's value it watches.
    pub metric: String,
    /// `>`, `>=`, `<` or `<=`.
    pub op: String,
    pub threshold: f64,
    /// How long the condition must hold before the alert fires, in
    /// seconds (on top of the window).
    #[serde(default)]
    pub for_s: u64,
    /// `critical`, `warning` or `info`.
    #[serde(default = "warning")]
    pub severity: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// How a window's points make one value: `mean`, `max`, `min`, `rate`
    /// (the change per second of a count) or `all` (every point must meet
    /// the condition: the least extreme value).
    #[serde(default = "mean")]
    pub eval: String,
    /// The window, in seconds.
    #[serde(default = "five_minutes")]
    pub window_s: u64,
    /// The threshold a firing alert resolves at (by default `threshold`):
    /// with `>` it resolves once the value is at most this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve: Option<f64>,
    /// `self` (this server's points) or `each_copy` (every copy's own).
    #[serde(default = "self_scope")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    /// A URL that says what to do.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runbook: String,
    /// The route its notifications go to (empty: every route that takes
    /// its severity).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub route: String,
    /// A coordinator's rule over the cluster's state ([`builtins`]): only
    /// its severity, `enabled` and route change.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builtin: bool,
}

fn warning() -> String {
    "warning".into()
}

fn yes() -> bool {
    true
}

fn mean() -> String {
    "mean".into()
}

fn five_minutes() -> u64 {
    300
}

fn self_scope() -> String {
    "self".into()
}

/// The longest window a rule takes: the series holds an hour.
pub const MAX_WINDOW_S: u64 = 3600;

impl Rule {
    fn check(&self) -> Result<(), ApiError> {
        if self.id.is_empty() || self.id.len() > 64 || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(ApiError::bad(format!("a rule's id is 1 to 64 letters, digits, '-' or '_', not {:?}", self.id)));
        }
        if self.name.trim().is_empty() {
            return Err(ApiError::bad(format!("rule {} needs a name", self.id)));
        }
        if !matches!(self.op.as_str(), ">" | ">=" | "<" | "<=") {
            return Err(ApiError::bad(format!("rule {}: op is >, >=, < or <=, not {:?}", self.id, self.op)));
        }
        if !matches!(self.severity.as_str(), "critical" | "warning" | "info") {
            return Err(ApiError::bad(format!("rule {}: severity is critical, warning or info, not {:?}", self.id, self.severity)));
        }
        if !matches!(self.eval.as_str(), "mean" | "max" | "min" | "rate" | "all") {
            return Err(ApiError::bad(format!("rule {}: eval is mean, max, min, rate or all, not {:?}", self.id, self.eval)));
        }
        if !matches!(self.scope.as_str(), "self" | "each_copy") {
            return Err(ApiError::bad(format!("rule {}: scope is self or each_copy, not {:?}", self.id, self.scope)));
        }
        if self.window_s == 0 || self.window_s > MAX_WINDOW_S {
            return Err(ApiError::bad(format!("rule {}: window_s is 1 to {MAX_WINDOW_S} seconds", self.id)));
        }
        if !self.threshold.is_finite() || self.resolve.is_some_and(|r| !r.is_finite()) {
            return Err(ApiError::bad(format!("rule {}: the threshold and resolve are numbers", self.id)));
        }
        if let Some(r) = self.resolve {
            let above = self.op.starts_with('>');
            if (above && r > self.threshold) || (!above && r < self.threshold) {
                return Err(ApiError::bad(format!("rule {}: it fires at {} {} and so resolves at {} the threshold or less extreme, not {r}", self.id, self.op, self.threshold, if above { "at most" } else { "at least" })));
            }
        }
        if !self.runbook.is_empty() && !(self.runbook.starts_with("https://") || self.runbook.starts_with("http://")) {
            return Err(ApiError::bad(format!("rule {}: the runbook is an http(s) URL", self.id)));
        }
        Ok(())
    }

    /// Whether `v` meets the condition.
    pub fn holds(&self, v: f64) -> bool {
        cmp(&self.op, v, self.threshold)
    }

    /// Whether a firing alert with value `v` still fires: `v` is past the
    /// resolve threshold.
    pub fn still(&self, v: f64) -> bool {
        cmp(&self.op, v, self.resolve.unwrap_or(self.threshold))
    }

    /// What decides when it fires and resolves; a change of it starts the
    /// alert again.
    fn condition(&self) -> (String, String, String, u64, u64, String, String, bool) {
        let n = |x: f64| format!("{x}");
        (self.metric.clone(), self.op.clone(), n(self.threshold), self.for_s, self.window_s, self.eval.clone(), self.resolve.map(n).unwrap_or_default() + &self.scope, self.enabled)
    }

    /// A short digest of [`Self::condition`]: a coordinator tells a copy
    /// that evaluates another version of the rule by it.
    pub fn sig(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let c = self.condition();
        (c.0, c.1, c.2, c.3, c.4, c.5, c.6).hash(&mut h);
        format!("{:08x}", h.finish() as u32)
    }

    fn copies(&self) -> bool {
        self.scope == "each_copy"
    }
}

fn cmp(op: &str, v: f64, t: f64) -> bool {
    match op {
        ">" => v > t,
        ">=" => v >= t,
        "<" => v < t,
        _ => v <= t,
    }
}

#[allow(clippy::too_many_arguments)]
fn rule(id: &str, name: &str, metric: &str, op: &str, threshold: f64, eval: &str, window_s: u64, for_s: u64, severity: &str, scope: &str, description: &str) -> Rule {
    Rule {
        id: id.into(),
        name: name.into(),
        metric: metric.into(),
        op: op.into(),
        threshold,
        for_s,
        severity: severity.into(),
        enabled: true,
        description: description.into(),
        eval: eval.into(),
        window_s,
        resolve: None,
        scope: scope.into(),
        owner: String::new(),
        runbook: String::new(),
        route: String::new(),
        builtin: false,
    }
}

/// The rules a server starts with. Those over a copy's own gauges (its
/// disk, writes, merges, replicas) are every copy's: a coordinator has
/// none of those gauges, and gathers its copies' states.
pub fn defaults() -> Vec<Rule> {
    let c = "each_copy";
    let s = "self";
    let mut v = vec![
        rule("failing", "Database refuses writes", "failing", ">", 0.0, "max", 30, 0, "critical", c, "A write failed partway (a full disk, an I/O error): restart the server once the cause is fixed."),
        rule("flush-failing", "Flushes fail", "flush_failing", ">", 0.0, "max", 30, 0, "critical", c, "Writes still commit to the log until the memtable holds twice its limit. Usually a full disk."),
        rule("merge-failing", "Merges fail", "merge_failing", ">", 0.0, "all", 60, 0, "warning", c, "Segments pile up and queries slow down until merges succeed again."),
        rule("disk-full", "Disk almost full", "disk_used_ratio", ">", 0.9, "mean", 300, 0, "warning", c, "Under 10% of the copy's disk is left."),
        rule("p99", "Query p99 over 1 s", "p99_ms", ">", 1000.0, "mean", 300, 0, "warning", s, "The slowest 1% of queries took over a second, on average over 5 minutes."),
        rule("errors", "Query errors over 5%", "error_ratio", ">", 0.05, "mean", 300, 0, "warning", s, "Over 5% of the queries that ended in 5 minutes failed on the server's side (errors and the server's deadline; not bad SQL nor a client's own deadline)."),
        rule("rejected", "Queries refused (429)", "rejected_s", ">", 0.5, "mean", 300, 0, "warning", s, "Queries found no slot (--max-concurrent) or no memory before their deadline, or a Contain rule refused them."),
        rule("5xx", "Server errors", "http_5xx_s", ">", 1.0, "mean", 300, 0, "warning", s, "Over one request a second failed on the server's side, over 5 minutes."),
        rule("replica-behind", "A replica is behind", "replica_behind_max", ">", 1000.0, "mean", 300, 0, "warning", c, "A replica misses over 1,000 transactions its primary committed."),
        rule("out-of-sync", "A replica is out of sync", "replicas_out_of_sync", ">", 0.0, "all", 60, 0, "critical", c, "A replica does not hold what its primary committed: a failover now could lose writes."),
        rule("replica-reads", "A replica does not serve reads", "serves_reads", "<", 1.0, "all", 60, 0, "warning", c, "The replica is catching up with its primary, or lost it."),
        rule("query-memory", "Query memory almost spent", "query_memory_ratio", ">", 0.9, "mean", 60, 0, "warning", s, "The running queries hold over 90% of --query-memory-mb; more are refused with 429."),
        rule("write-queue", "Writes queue up", "write_queue", ">", 512.0, "mean", 60, 0, "warning", c, "Write requests wait for the writer; at --max-queued-writes they are refused."),
    ];
    v[3].resolve = Some(0.88);
    v[5].resolve = Some(0.03);
    v
}

/// A coordinator's rules over its cluster's state, besides its defaults.
/// Their metrics are the coordinator's points (`partial_s`,
/// `availability`) or its view of the cluster ([`ClusterView`]).
pub fn builtins() -> Vec<Rule> {
    let s = "self";
    let mut v = vec![
        rule("shard-no-majority", "A shard has no majority", "shards_no_majority", ">", 0.0, "max", 10, 0, "critical", s, "Fewer than half of a shard's copies answer: with failover its primary serves no reads, so every search fails. Bring a copy back, or remove the dead one."),
        rule("copy-down", "A copy is down", "copies_down", ">", 0.0, "max", 10, 30, "warning", s, "A copy of a shard has not answered the coordinator for 30 seconds."),
        rule("under-replicated", "A shard is under-replicated", "shards_under_replicated", ">", 0.0, "max", 10, 60, "warning", s, "A shard has fewer copies in sync and serving than it has copies, for a minute."),
        rule("primary-changed", "A shard's primary changed", "primary_changes", ">", 0.0, "max", 10, 0, "info", s, "A shard elected another primary in the last 10 minutes; the alert resolves 10 minutes after the last change."),
        rule("partial-answers", "Partial answers", "partial_s", ">", 0.0, "min", 60, 0, "warning", s, "For a minute, some answers left out shards that did not answer."),
        rule("copy-silent", "A copy stopped reporting", "copies_silent", ">", 0.0, "max", 10, 0, "warning", s, "A copy answers but its own numbers are over 60 seconds old."),
        rule("availability", "Searches fail", "availability", "<", 0.99, "mean", 300, 0, "critical", s, "Under 99% of the searches that ended in 5 minutes succeeded (failures on the server's side)."),
    ];
    for r in &mut v {
        r.builtin = true;
    }
    v
}

/// Metrics a coordinator computes from its cluster's state.
pub const CLUSTER_METRICS: [&str; 5] = ["shards_no_majority", "copies_down", "shards_under_replicated", "primary_changes", "copies_silent"];

/// An acknowledgement: someone is on it.
#[derive(Clone, Debug, Serialize)]
pub struct Ack {
    pub by: String,
    pub at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Notifications held back until `until_ms`.
#[derive(Clone, Debug, Serialize)]
pub struct Silence {
    pub by: String,
    pub until_ms: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Note {
    pub by: String,
    pub at_ms: u64,
    pub text: String,
}

/// Notes kept per rule.
pub const NOTES: usize = 100;

/// A rule's state now.
#[derive(Clone, Debug)]
struct St {
    /// `ok`, `pending`, `firing` or `no_data`.
    state: &'static str,
    since_ms: u64,
    value: Option<f64>,
    /// Since when the window holds no value, if it holds none now.
    no_data_since_ms: Option<u64>,
    ack: Option<Ack>,
    silenced: Option<Silence>,
    notes: Vec<Note>,
    /// What is not ok: copies, shards.
    where_: Vec<Json>,
    /// An each_copy rule's copies, by state.
    copies: Option<Json>,
}

impl St {
    fn new(t: u64) -> St {
        St { state: "no_data", since_ms: t, value: None, no_data_since_ms: Some(t), ack: None, silenced: None, notes: Vec::new(), where_: Vec::new(), copies: None }
    }

    /// Takes the window's value `v` at `t`: returns "firing" or
    /// "resolved" when the alert changes so.
    fn step(&mut self, r: &Rule, v: Option<f64>, t: u64) -> Option<&'static str> {
        let Some(v) = v else {
            // No data keeps the state; it never resolves.
            self.no_data_since_ms.get_or_insert(t);
            if self.state == "ok" {
                (self.state, self.since_ms) = ("no_data", t);
            }
            return None;
        };
        self.no_data_since_ms = None;
        self.value = Some(v);
        let was = self.state;
        let next = match was {
            "firing" if r.still(v) => "firing",
            "firing" => "ok",
            "pending" if r.holds(v) && t.saturating_sub(self.since_ms) >= r.for_s * 1000 => "firing",
            "pending" if r.holds(v) => "pending",
            _ if r.holds(v) && r.for_s == 0 => "firing",
            _ if r.holds(v) => "pending",
            _ => "ok",
        };
        if next != was {
            (self.state, self.since_ms) = (next, t);
        }
        match (was, next) {
            ("firing", "firing") => None,
            (_, "firing") => Some("firing"),
            ("firing", _) => {
                self.ack = None;
                Some("resolved")
            }
            _ => None,
        }
    }
}

fn state_rank(s: &str) -> u8 {
    match s {
        "firing" => 0,
        "pending" => 1,
        "no_data" => 2,
        "ok" => 3,
        _ => 4,
    }
}

/// A change of an alert: `firing`, `resolved`, `acked`, `silenced`,
/// `unsilenced` or `noted`.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub ts_ms: u64,
    pub rule: String,
    pub name: String,
    pub severity: String,
    pub state: &'static str,
    pub value: Option<f64>,
    pub threshold: f64,
    pub op: String,
    pub metric: String,
    pub eval: String,
    pub window_s: u64,
    /// Short names of what is not ok (copies, shards).
    #[serde(rename = "where", skip_serializing_if = "Vec::is_empty")]
    pub where_: Vec<String>,
    /// What is not ok, as the state's `where` has it (20 at most).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub place: Vec<Json>,
    /// Why it resolved without its value: "the rule was edited".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// A note's text, a silence's reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// When a resolved alert fired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fired_ms: Option<u64>,
    /// Silenced: not posted to the routes.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub silenced: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub runbook: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub route: String,
}

impl Event {
    fn of(r: &Rule, state: &'static str, st: &St, t: u64) -> Event {
        Event {
            ts_ms: t,
            rule: r.id.clone(),
            name: r.name.clone(),
            severity: r.severity.clone(),
            state,
            value: st.value,
            threshold: if state == "resolved" { r.resolve.unwrap_or(r.threshold) } else { r.threshold },
            op: r.op.clone(),
            metric: r.metric.clone(),
            eval: r.eval.clone(),
            window_s: r.window_s,
            where_: st.where_.iter().filter_map(|w| w["short"].as_str().or_else(|| w["name"].as_str()).map(String::from)).collect(),
            place: st.where_.iter().take(20).cloned().collect(),
            why: None,
            by: None,
            text: None,
            fired_ms: None,
            silenced: st.silenced.as_ref().is_some_and(|s| s.until_ms > now_ms()),
            description: r.description.clone(),
            runbook: r.runbook.clone(),
            owner: r.owner.clone(),
            route: r.route.clone(),
        }
    }

    /// Whether it is posted to routes: firing and resolved, unless
    /// silenced.
    pub fn notifies(&self) -> bool {
        matches!(self.state, "firing" | "resolved") && !self.silenced
    }
}

/// Events kept in memory.
pub const EVENTS: usize = 2000;

/// Where notifications go: a URL taking `{"text", "alert"}` (Slack's
/// incoming webhooks take it as is), for the severities listed (all when
/// empty).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub severities: Vec<String>,
}

impl Route {
    fn takes(&self, severity: &str) -> bool {
        self.severities.is_empty() || self.severities.iter().any(|s| s == severity)
    }

    /// The URL with its path left out: a webhook's path is its secret.
    fn shown_url(&self) -> String {
        match self.url.find("://").map(|i| i + 3) {
            Some(i) => {
                let host_end = self.url[i..].find('/').map_or(self.url.len(), |j| i + j);
                let more = if host_end < self.url.len() { "/..." } else { "" };
                format!("{}{more}", &self.url[..host_end])
            }
            None => String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
struct Delivery {
    sent: u64,
    failed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_ok_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error_ms: Option<u64>,
}

/// What the rules file holds.
#[derive(Default, Serialize, Deserialize)]
struct Kept {
    rules: Vec<Rule>,
    #[serde(default)]
    routes: Vec<Route>,
}

/// One copy's state of an each_copy rule, as the coordinator last heard.
#[derive(Clone, Debug)]
struct CopySt {
    state: String,
    value: Option<f64>,
    since_ms: u64,
    short: String,
    shard: usize,
    /// The copy evaluates another version of the rule.
    differs: bool,
    /// Since when it has not reported (down, or reporting no state).
    unheard_ms: Option<u64>,
}

/// The cluster as the built-in rules see it: one row a check.
#[derive(Default)]
struct ClusterView {
    /// (time, metric values) of each check, the oldest first.
    points: VecDeque<(u64, HashMap<&'static str, f64>)>,
    /// Each built-in metric's `where`, from the latest check.
    places: HashMap<&'static str, Vec<Json>>,
    /// Each shard's primary as last seen, and when primaries changed.
    primaries: HashMap<usize, String>,
    changes: VecDeque<(u64, Json)>,
}

/// How long a primary change keeps `primary-changed` firing.
const PRIMARY_CHANGED_MS: u64 = 600_000;
/// A copy whose latest point is older than this is silent.
const SILENT_MS: u64 = 60_000;

pub struct Alerts {
    rules: Mutex<Vec<Rule>>,
    states: Mutex<HashMap<String, St>>,
    events: Mutex<VecDeque<Arc<Event>>>,
    routes: Mutex<Vec<Route>>,
    deliveries: Mutex<HashMap<String, Delivery>>,
    file: Option<PathBuf>,
    http: reqwest::Client,
    coordinator: AtomicBool,
    copies: Mutex<HashMap<(String, String), CopySt>>,
    cluster: Mutex<ClusterView>,
    /// When the coordinator last checked its cluster (ms), and whether a
    /// check runs.
    pub(crate) cluster_checked: AtomicU64,
    pub(crate) cluster_running: AtomicBool,
    identity: Mutex<Identity>,
}

/// What notifications say of where they come from.
#[derive(Clone, Debug, Default)]
pub struct Identity {
    /// The cluster's name (`--cluster-name`), else this server's.
    pub name: String,
    /// The command center's address, for links (`--public-url`).
    pub public_url: Option<String>,
}

impl Alerts {
    /// The rules and routes of `file` ([`defaults`] and no route if it
    /// does not exist), with a route "default" to `webhook` if given.
    pub fn new(file: Option<PathBuf>, webhook: Option<String>) -> Result<Alerts, String> {
        let mut kept = match &file {
            Some(p) => match std::fs::read(p) {
                Ok(b) => {
                    let j: Json = serde_json::from_slice(&b).map_err(|e| format!("--alerts {}: {e}", p.display()))?;
                    // A list of rules, as files held before routes.
                    let kept: Kept = match j {
                        Json::Array(_) => Kept { rules: serde_json::from_value(j).map_err(|e| format!("--alerts {}: {e}", p.display()))?, routes: Vec::new() },
                        j => serde_json::from_value(j).map_err(|e| format!("--alerts {}: {e}", p.display()))?,
                    };
                    for r in &kept.rules {
                        r.check().map_err(|e| format!("--alerts {}: {}", p.display(), e.message))?;
                    }
                    kept
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Kept { rules: defaults(), routes: Vec::new() },
                Err(e) => return Err(format!("--alerts {}: {e}", p.display())),
            },
            None => Kept { rules: defaults(), routes: Vec::new() },
        };
        if let Some(url) = webhook.filter(|u| !u.is_empty()) {
            kept.routes.retain(|r| r.name != "default");
            kept.routes.insert(0, Route { name: "default".into(), url, severities: Vec::new() });
        }
        let http = crate::http_client().timeout(std::time::Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
        // The service's (`--cluster-name`, `--public-url`) once it runs
        // ([`deliver`]); this machine's name until then.
        let identity = Identity { name: crate::logstore::hostname(), public_url: None };
        Ok(Alerts {
            rules: Mutex::new(kept.rules),
            states: Mutex::new(HashMap::new()),
            events: Mutex::new(VecDeque::new()),
            routes: Mutex::new(kept.routes),
            deliveries: Mutex::new(HashMap::new()),
            file,
            http,
            coordinator: AtomicBool::new(false),
            copies: Mutex::new(HashMap::new()),
            cluster: Mutex::new(ClusterView::default()),
            cluster_checked: AtomicU64::new(0),
            cluster_running: AtomicBool::new(false),
            identity: Mutex::new(identity),
        })
    }

    /// Names the cluster and the command center's address in
    /// notifications.
    pub fn set_identity(&self, name: Option<String>, public_url: Option<String>) {
        let mut i = lock(&self.identity);
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            i.name = n;
        }
        if let Some(u) = public_url.filter(|u| !u.is_empty()) {
            i.public_url = Some(u.trim_end_matches('/').to_string());
        }
    }

    pub fn identity(&self) -> Identity {
        lock(&self.identity).clone()
    }

    /// Whether this server coordinates shards: it then has the
    /// [`builtins`], and gathers its copies' states of each_copy rules
    /// rather than evaluating them.
    pub fn set_coordinator(&self, yes: bool) {
        if self.coordinator.swap(yes, Ordering::Relaxed) == yes {
            return;
        }
        let mut rules = lock(&self.rules);
        if yes {
            for b in builtins() {
                if !rules.iter().any(|r| r.id == b.id) {
                    rules.push(b);
                }
            }
        }
        // Built-in rules kept in a file take the definitions of this
        // version, but for what may change.
        let defs = builtins();
        for r in rules.iter_mut().filter(|r| r.builtin) {
            if let Some(d) = defs.iter().find(|d| d.id == r.id) {
                *r = Rule { severity: r.severity.clone(), enabled: r.enabled, route: r.route.clone(), ..d.clone() };
            }
        }
        if !yes {
            rules.retain(|r| !r.builtin);
        }
    }

    pub fn is_coordinator(&self) -> bool {
        self.coordinator.load(Ordering::Relaxed)
    }

    pub fn rules(&self) -> Vec<Rule> {
        lock(&self.rules).clone()
    }

    pub fn file(&self) -> Option<&std::path::Path> {
        self.file.as_deref()
    }

    /// Whether any route is set.
    pub fn webhook(&self) -> bool {
        !lock(&self.routes).is_empty()
    }

    pub fn routes(&self) -> Vec<Route> {
        lock(&self.routes).clone()
    }

    fn save(&self, rules: &[Rule], routes: &[Route]) -> Result<(), ApiError> {
        let Some(p) = &self.file else { return Ok(()) };
        let tmp = p.with_extension("tmp");
        let body = serde_json::to_vec_pretty(&Kept { rules: rules.to_vec(), routes: routes.to_vec() }).map_err(ApiError::internal)?;
        std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, p)).map_err(|e| ApiError::internal(format!("write {}: {e}", p.display())))
    }

    /// Replaces the rules (and their file), keeping the states of the
    /// rules whose condition stays the same. A built-in rule takes only
    /// its severity, `enabled` and route, and is never removed. An alert
    /// that fired and whose rule changed, went or was disabled resolves:
    /// its events are returned, with the rules before and after.
    pub fn set_rules(&self, rules: Vec<Rule>, by: &str) -> Result<(Vec<Arc<Event>>, Vec<Rule>, Vec<Rule>), ApiError> {
        let mut ids = HashSet::new();
        for r in &rules {
            r.check()?;
            if !ids.insert(r.id.clone()) {
                return Err(ApiError::bad(format!("two rules have the id {:?}", r.id)));
            }
        }
        let before = self.rules();
        let mut next: Vec<Rule> = Vec::new();
        for r in rules {
            match before.iter().find(|b| b.id == r.id && b.builtin) {
                Some(b) => next.push(Rule { severity: r.severity, enabled: r.enabled, route: r.route, ..b.clone() }),
                None if builtins().iter().any(|b| b.id == r.id) && self.is_coordinator() => return Err(ApiError::bad(format!("{} is the id of a built-in rule", r.id))),
                None => next.push(Rule { builtin: false, ..r }),
            }
        }
        for b in before.iter().filter(|b| b.builtin) {
            if !next.iter().any(|r| r.id == b.id) {
                next.push(b.clone());
            }
        }
        self.save(&next, &self.routes())?;
        let t = now_ms();
        let mut events = Vec::new();
        {
            let mut now = lock(&self.rules);
            let mut states = lock(&self.states);
            for old in before.iter() {
                let new = next.iter().find(|r| r.id == old.id);
                let same = new.is_some_and(|n| n.condition() == old.condition());
                if same {
                    continue;
                }
                if let Some(st) = states.remove(&old.id) {
                    if st.state == "firing" {
                        let why = match new {
                            None => "the rule was deleted",
                            Some(n) if !n.enabled => "the rule was disabled",
                            Some(_) => "the rule was edited",
                        };
                        let mut e = Event::of(old, "resolved", &st, t);
                        e.why = Some(why.into());
                        e.by = Some(by.to_string());
                        e.fired_ms = Some(st.since_ms);
                        events.push(Arc::new(e));
                    }
                    // Notes, acknowledgements and silences stay with an
                    // edited rule.
                    if let Some(n) = new {
                        let mut fresh = St::new(t);
                        (fresh.notes, fresh.silenced) = (st.notes, st.silenced);
                        states.insert(n.id.clone(), fresh);
                    }
                }
                lock(&self.copies).retain(|(r, _), _| r != &old.id);
            }
            *now = next.clone();
        }
        self.keep(&events);
        Ok((events, before, next))
    }

    /// Replaces the routes; a route given without a URL keeps the URL
    /// its name had.
    pub fn set_routes(&self, routes: Vec<Route>) -> Result<Vec<Route>, ApiError> {
        let before = self.routes();
        let mut names = HashSet::new();
        let mut next = Vec::new();
        for mut r in routes {
            if r.name.is_empty() || r.name.len() > 64 || !r.name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                return Err(ApiError::bad(format!("a route's name is 1 to 64 letters, digits, '-' or '_', not {:?}", r.name)));
            }
            if !names.insert(r.name.clone()) {
                return Err(ApiError::bad(format!("two routes are named {:?}", r.name)));
            }
            if r.url.is_empty() {
                r.url = before.iter().find(|b| b.name == r.name).map(|b| b.url.clone()).ok_or_else(|| ApiError::bad(format!("route {} needs a url", r.name)))?;
            }
            if !(r.url.starts_with("https://") || r.url.starts_with("http://")) {
                return Err(ApiError::bad(format!("route {}: the url is http(s)", r.name)));
            }
            if let Some(s) = r.severities.iter().find(|s| !matches!(s.as_str(), "critical" | "warning" | "info")) {
                return Err(ApiError::bad(format!("route {}: severities are critical, warning and info, not {s:?}", r.name)));
            }
            next.push(r);
        }
        self.save(&self.rules(), &next)?;
        *lock(&self.routes) = next.clone();
        lock(&self.deliveries).retain(|n, _| next.iter().any(|r| &r.name == n));
        Ok(before)
    }

    /// The rules this server evaluates on its own points: all of them, but
    /// a coordinator's each_copy ones (its copies evaluate them) and its
    /// rules over the cluster.
    fn own(&self, r: &Rule) -> bool {
        r.enabled && !(self.is_coordinator() && (r.copies() || CLUSTER_METRICS.contains(&r.metric.as_str())))
    }

    /// Evaluates this server's rules on its points (`points`, the oldest
    /// first, with the latest at `t`); returns the changes, which it keeps
    /// and logs.
    pub fn check(&self, points: &[Arc<Json>], t: u64) -> Vec<Arc<Event>> {
        let rules = self.rules();
        let mut changes = Vec::new();
        {
            let mut states = lock(&self.states);
            for r in rules.iter().filter(|r| self.own(r)) {
                let v = window_value(r, points, t);
                let st = states.entry(r.id.clone()).or_insert_with(|| St::new(t));
                if let Some(change) = st.step(r, v, t) {
                    let mut e = Event::of(r, change, st, t);
                    if change == "resolved" {
                        e.fired_ms = Some(self.fired_at(&r.id).unwrap_or(t));
                    }
                    changes.push(Arc::new(e));
                }
            }
            // Rules removed or disabled have no state but their notes.
            states.retain(|id, s| rules.iter().any(|r| r.enabled && &r.id == id) || !s.notes.is_empty());
        }
        self.keep(&changes);
        changes
    }

    /// When the alert of rule `id` last fired, from the events.
    fn fired_at(&self, id: &str) -> Option<u64> {
        lock(&self.events).iter().rev().find(|e| e.rule == id && e.state == "firing").map(|e| e.ts_ms)
    }

    /// Keeps events and logs the changes.
    fn keep(&self, changes: &[Arc<Event>]) {
        if changes.is_empty() {
            return;
        }
        let mut events = lock(&self.events);
        for e in changes {
            if events.len() == EVENTS {
                events.pop_front();
            }
            events.push_back(e.clone());
            // The plain number, or "no data" (never `Some(0.036)`).
            let value = e.value.map_or_else(|| "no data".to_string(), number);
            match e.state {
                "firing" => tracing::warn!(rule = %e.rule, severity = %e.severity, value = %value, threshold = %number(e.threshold), place = %e.where_.join(", "), "alert firing: {}", e.name),
                "resolved" => tracing::info!(rule = %e.rule, value = %value, why = e.why.as_deref().unwrap_or(""), "alert resolved: {}", e.name),
                _ => tracing::info!(rule = %e.rule, by = e.by.as_deref().unwrap_or(""), "alert {}: {}", e.state, e.name),
            }
        }
    }

    /// A coordinator's check of its cluster's state (`/v1/cluster`'s
    /// answer): the built-in rules over it, and its copies' states of
    /// each_copy rules gathered.
    pub fn check_cluster(&self, snap: &Json, t: u64) -> Vec<Arc<Event>> {
        let rules = self.rules();
        let mut changes = Vec::new();
        let (values, places) = self.view(snap, t);
        {
            let mut c = lock(&self.cluster);
            c.points.push_back((t, values));
            while c.points.front().is_some_and(|(pt, _)| *pt + MAX_WINDOW_S * 1000 < t) {
                c.points.pop_front();
            }
            c.places = places;
        }
        // The built-in rules over the cluster's view.
        {
            let c = lock(&self.cluster);
            let mut states = lock(&self.states);
            for r in rules.iter().filter(|r| r.enabled && CLUSTER_METRICS.contains(&r.metric.as_str())) {
                let from = t.saturating_sub(r.window_s * 1000);
                let vals: Vec<(u64, f64)> = c.points.iter().filter(|(pt, _)| *pt > from && *pt <= t).filter_map(|(pt, m)| m.get(r.metric.as_str()).map(|v| (*pt, *v))).collect();
                let v = aggregate(r, &vals);
                let st = states.entry(r.id.clone()).or_insert_with(|| St::new(t));
                st.where_ = c.places.get(r.metric.as_str()).cloned().unwrap_or_default();
                if let Some(change) = st.step(r, v, t) {
                    changes.push(Arc::new(Event::of(r, change, st, t)));
                }
            }
        }
        changes.extend(self.gather(snap, &rules, t));
        self.keep(&changes);
        changes
    }

    /// The built-in metrics of a cluster's state, and what each names.
    fn view(&self, snap: &Json, t: u64) -> (HashMap<&'static str, f64>, HashMap<&'static str, Vec<Json>>) {
        let mut places: HashMap<&'static str, Vec<Json>> = HashMap::new();
        let shards = snap["shards"].as_array().cloned().unwrap_or_default();
        let mut c = lock(&self.cluster);
        for s in &shards {
            let i = s["shard"].as_u64().unwrap_or(0) as usize;
            let replicas = s["replicas"].as_array().cloned().unwrap_or_default();
            let up: Vec<&Json> = replicas.iter().filter(|r| answers(r)).collect();
            let n = replicas.len();
            // The snapshot's `majority` (core's shard state), else from the
            // copies that answer. Without failover a shard serves reads
            // without a majority: that is copy-down's, not this rule's.
            let majority = s["majority"].as_bool().unwrap_or(n > 0 && up.len() * 2 > n);
            let failover = s["failover"].as_bool().unwrap_or(true);
            let shard_name = format!("shard {i}");
            if !majority && failover {
                let why = s["why"].as_str().map(String::from).unwrap_or_else(|| format!("{} of {n} copies answer", up.len()));
                places.entry("shards_no_majority").or_default().push(json!({"shard": i, "name": shard_name, "short": shard_name, "state": "firing", "why": why}));
            }
            // In sync and serving: answers, serves reads, and its primary
            // counts no replica out of sync.
            let healthy = up.iter().filter(|r| r["serves_reads"] != false && r["detail"]["load"]["serves_reads"].as_f64() != Some(0.0)).count();
            let out_of_sync = up.iter().filter_map(|r| r["detail"]["load"]["replicas_out_of_sync"].as_f64()).fold(0.0, f64::max) as usize;
            let in_sync = healthy.saturating_sub(out_of_sync);
            if n > 0 && in_sync < n {
                places.entry("shards_under_replicated").or_default().push(json!({"shard": i, "name": shard_name, "short": shard_name, "state": "firing", "value": in_sync, "of": n}));
            }
            for (k, r) in replicas.iter().enumerate() {
                let short = r["short"].as_str().map(String::from).unwrap_or_else(|| format!("s{i}-{k}"));
                let url = r["url"].as_str().unwrap_or_default().to_string();
                if !answers(r) {
                    let since = r["down_since_ms"].as_u64();
                    places.entry("copies_down").or_default().push(json!({"copy": url, "short": short, "shard": i, "state": "firing", "since_ms": since, "error": r["error"]}));
                } else {
                    let seen = r["detail"]["load"]["t"].as_u64().or_else(|| r["last_seen_ms"].as_u64());
                    if seen.is_none_or(|s| t.saturating_sub(s) > SILENT_MS) {
                        places.entry("copies_silent").or_default().push(json!({"copy": url, "short": short, "shard": i, "state": "firing", "last_ms": seen}));
                    }
                }
            }
            // The primary: the field, else the copy that answers as one
            // and takes writes.
            let primary = s["primary"].as_str().map(String::from).or_else(|| up.iter().find(|r| r["role"] == "primary" && r["takes_writes"] != false).and_then(|r| r["url"].as_str()).map(String::from));
            if let Some(p) = primary {
                let was = c.primaries.insert(i, p.clone());
                if let Some(w) = was.filter(|w| *w != p) {
                    c.changes.push_back((t, json!({"shard": i, "name": shard_name, "short": shard_name, "state": "firing", "from": w, "to": p, "at_ms": t})));
                }
            }
        }
        while c.changes.front().is_some_and(|(ct, _)| ct + PRIMARY_CHANGED_MS < t) {
            c.changes.pop_front();
        }
        if !c.changes.is_empty() {
            places.insert("primary_changes", c.changes.iter().map(|(_, j)| j.clone()).collect());
        }
        let mut values = HashMap::new();
        if !shards.is_empty() {
            for m in CLUSTER_METRICS {
                values.insert(m, places.get(m).map_or(0, Vec::len) as f64);
            }
        }
        (values, places)
    }

    /// Gathers the copies' states of the each_copy rules from their
    /// detail (`detail.alerts.rules`): a copy that stops reporting keeps
    /// its last state, and the rule's state is the worst copy's.
    fn gather(&self, snap: &Json, rules: &[Rule], t: u64) -> Vec<Arc<Event>> {
        let mut changes = Vec::new();
        let mut copies = lock(&self.copies);
        let mut present = HashSet::new();
        for s in snap["shards"].as_array().into_iter().flatten() {
            let i = s["shard"].as_u64().unwrap_or(0) as usize;
            for (k, r) in s["replicas"].as_array().into_iter().flatten().enumerate() {
                let url = r["url"].as_str().unwrap_or_default().to_string();
                let short = r["short"].as_str().map(String::from).unwrap_or_else(|| format!("s{i}-{k}"));
                let reported = &r["detail"]["alerts"]["rules"];
                for rule in rules.iter().filter(|r| r.enabled && r.copies()) {
                    present.insert((rule.id.clone(), url.clone()));
                    let key = (rule.id.clone(), url.clone());
                    let mine = &reported[&rule.id];
                    let e = copies.entry(key).or_insert_with(|| CopySt { state: "no_data".into(), value: None, since_ms: t, short: short.clone(), shard: i, differs: false, unheard_ms: Some(t) });
                    e.short = short.clone();
                    match mine["state"].as_str() {
                        Some(state) => {
                            e.state = state.to_string();
                            e.value = mine["value"].as_f64();
                            e.since_ms = mine["since_ms"].as_u64().unwrap_or(t);
                            e.differs = mine["sig"].as_str().is_some_and(|s| s != rule.sig());
                            e.unheard_ms = None;
                        }
                        None => {
                            e.unheard_ms.get_or_insert(t);
                        }
                    }
                }
            }
        }
        copies.retain(|k, _| present.contains(k));
        let mut states = lock(&self.states);
        for rule in rules.iter().filter(|r| r.enabled && r.copies()) {
            let mine: Vec<&CopySt> = copies.iter().filter(|((r, _), _)| r == &rule.id).map(|(_, c)| c).collect();
            let urls: Vec<&String> = copies.iter().filter(|((r, _), _)| r == &rule.id).map(|((_, u), _)| u).collect();
            let count = |s: &str| mine.iter().filter(|c| c.state == s).count();
            let worst = mine.iter().map(|c| state_rank(&c.state)).min().unwrap_or(2);
            let state: &'static str = match worst {
                0 => "firing",
                1 => "pending",
                3 if count("no_data") == 0 => "ok",
                3 => "ok",
                _ => "no_data",
            };
            let st = states.entry(rule.id.clone()).or_insert_with(|| St::new(t));
            let was = st.state;
            let in_state: Vec<usize> = (0..mine.len()).filter(|&j| state_rank(&mine[j].state) == worst).collect();
            st.value = in_state.iter().filter_map(|&j| mine[j].value).reduce(|a, b| if rule.op.starts_with('>') { a.max(b) } else { a.min(b) });
            let mut place: Vec<(u8, Json)> = (0..mine.len())
                .filter(|&j| mine[j].state != "ok" || mine[j].differs)
                .map(|j| {
                    let c = mine[j];
                    let mut w = json!({"copy": urls[j], "short": c.short, "shard": c.shard, "state": c.state, "value": c.value, "since_ms": c.since_ms});
                    if c.differs {
                        w["rule_differs"] = json!(true);
                    }
                    if let Some(u) = c.unheard_ms {
                        w["unheard_since_ms"] = json!(u);
                    }
                    (state_rank(&c.state), w)
                })
                .collect();
            place.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1["short"].as_str().cmp(&b.1["short"].as_str())));
            st.where_ = place.into_iter().map(|(_, w)| w).collect();
            st.copies = Some(json!({"total": mine.len(), "firing": count("firing"), "pending": count("pending"), "no_data": count("no_data"), "ok": count("ok"), "rule_differs": mine.iter().filter(|c| c.differs).count()}));
            if state != was {
                st.since_ms = in_state.iter().map(|&j| mine[j].since_ms).min().unwrap_or(t);
                st.state = state;
                st.no_data_since_ms = (state == "no_data").then_some(t);
                let change = match (was, state) {
                    (_, "firing") => Some("firing"),
                    ("firing", _) => Some("resolved"),
                    _ => None,
                };
                if let Some(c) = change {
                    if c == "resolved" {
                        st.ack = None;
                    }
                    let mut e = Event::of(rule, c, st, t);
                    if c == "resolved" {
                        e.fired_ms = self.fired_at(&rule.id);
                    }
                    changes.push(Arc::new(e));
                }
            }
        }
        changes
    }

    /// Acknowledges rule `id`'s alert, which fires or is pending.
    pub fn ack(&self, id: &str, by: &str, note: Option<String>) -> Result<Arc<Event>, ApiError> {
        let r = self.rule(id)?;
        let t = now_ms();
        let mut states = lock(&self.states);
        let st = states.get_mut(id).filter(|s| matches!(s.state, "firing" | "pending")).ok_or_else(|| ApiError { status: 409, message: format!("{id} is not firing nor pending: there is nothing to acknowledge") })?;
        st.ack = Some(Ack { by: by.into(), at_ms: t, note: note.clone() });
        if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
            push_note(st, by, t, n);
        }
        let mut e = Event::of(&r, "acked", st, t);
        (e.by, e.text) = (Some(by.into()), st.ack.as_ref().and_then(|a| a.note.clone()));
        drop(states);
        let e = Arc::new(e);
        self.keep(std::slice::from_ref(&e));
        Ok(e)
    }

    /// Holds back rule `id`'s notifications for `minutes`.
    pub fn silence(&self, id: &str, by: &str, minutes: u64, reason: &str) -> Result<Arc<Event>, ApiError> {
        if minutes == 0 || minutes > 7 * 24 * 60 {
            return Err(ApiError::bad("minutes is 1 to 10080 (a week)"));
        }
        if reason.trim().is_empty() {
            return Err(ApiError::bad("say why it is silenced: the body needs a \"reason\""));
        }
        let r = self.rule(id)?;
        let t = now_ms();
        let mut states = lock(&self.states);
        let st = states.entry(id.to_string()).or_insert_with(|| St::new(t));
        st.silenced = Some(Silence { by: by.into(), until_ms: t + minutes * 60_000, reason: reason.into() });
        let mut e = Event::of(&r, "silenced", st, t);
        (e.by, e.text) = (Some(by.into()), Some(format!("for {minutes} min: {reason}")));
        drop(states);
        let e = Arc::new(e);
        self.keep(std::slice::from_ref(&e));
        Ok(e)
    }

    pub fn unsilence(&self, id: &str, by: &str) -> Result<Arc<Event>, ApiError> {
        let r = self.rule(id)?;
        let t = now_ms();
        let mut states = lock(&self.states);
        let st = states.get_mut(id).filter(|s| s.silenced.is_some()).ok_or_else(|| ApiError { status: 404, message: format!("{id} is not silenced") })?;
        st.silenced = None;
        let mut e = Event::of(&r, "unsilenced", st, t);
        e.by = Some(by.into());
        drop(states);
        let e = Arc::new(e);
        self.keep(std::slice::from_ref(&e));
        Ok(e)
    }

    /// Adds a note to rule `id`'s alert.
    pub fn note(&self, id: &str, by: &str, text: &str) -> Result<Arc<Event>, ApiError> {
        let text = text.trim();
        if text.is_empty() || text.len() > 4000 {
            return Err(ApiError::bad("a note is 1 to 4,000 characters of text"));
        }
        let r = self.rule(id)?;
        let t = now_ms();
        let mut states = lock(&self.states);
        let st = states.entry(id.to_string()).or_insert_with(|| St::new(t));
        push_note(st, by, t, text.to_string());
        let mut e = Event::of(&r, "noted", st, t);
        (e.by, e.text) = (Some(by.into()), Some(text.into()));
        drop(states);
        let e = Arc::new(e);
        self.keep(std::slice::from_ref(&e));
        Ok(e)
    }

    fn rule(&self, id: &str) -> Result<Rule, ApiError> {
        self.rules().into_iter().find(|r| r.id == id).ok_or_else(|| ApiError { status: 404, message: format!("no alert rule {id}") })
    }

    /// Rule `r`'s state as the API answers it.
    fn state_json(&self, r: &Rule, st: Option<&St>) -> Json {
        let now = now_ms();
        let mut j = json!({
            "rule": r.id, "name": r.name, "severity": r.severity, "scope": r.scope, "builtin": r.builtin,
            "metric": r.metric, "op": r.op, "threshold": r.threshold, "eval": r.eval, "window_s": r.window_s, "for_s": r.for_s,
            "resolve": r.resolve.unwrap_or(r.threshold), "description": r.description, "owner": r.owner, "runbook": r.runbook, "route": r.route,
        });
        match (r.enabled, st) {
            (false, _) => j["state"] = json!("disabled"),
            (true, None) => {
                j["state"] = json!(if self.is_coordinator() && r.copies() { "no_data" } else { "no_data" });
            }
            (true, Some(s)) => {
                j["state"] = json!(s.state);
                j["since_ms"] = json!(s.since_ms);
                j["value"] = json!(s.value);
                if let Some(n) = s.no_data_since_ms {
                    j["no_data_since_ms"] = json!(n);
                }
                j["where"] = json!(s.where_);
                if let Some(c) = &s.copies {
                    j["copies"] = c.clone();
                }
                if let Some(a) = &s.ack {
                    j["ack"] = json!(a);
                }
            }
        }
        if let Some(s) = st {
            if let Some(si) = s.silenced.as_ref().filter(|si| si.until_ms > now) {
                j["silenced"] = json!(si);
            }
            j["notes"] = json!(s.notes);
        } else {
            j["notes"] = json!([]);
        }
        if j.get("where").is_none() {
            j["where"] = json!([]);
        }
        j
    }

    /// The rules, their states (firing first, then pending, no data, ok),
    /// the latest events (the newest first), the routes and their
    /// deliveries.
    pub fn json(&self) -> Json {
        let rules = self.rules();
        let states = lock(&self.states).clone();
        let mut out: Vec<Json> = rules.iter().map(|r| self.state_json(r, states.get(&r.id))).collect();
        out.sort_by_key(|s| (state_rank(s["state"].as_str().unwrap_or("")), match s["severity"].as_str() {
            Some("critical") => 0,
            Some("warning") => 1,
            _ => 2,
        }));
        // The rules as before, each with its state.
        let with: Vec<Json> = rules
            .iter()
            .map(|r| {
                let mut j = json!(r);
                let s = out.iter().find(|s| s["rule"] == r.id.as_str()).cloned().unwrap_or(Json::Null);
                j["status"] = json!({"state": s["state"], "since_ms": s["since_ms"], "value": s["value"]});
                j
            })
            .collect();
        let count = |st: &str| out.iter().filter(|s| s["state"] == st).count();
        let events: Vec<Arc<Event>> = lock(&self.events).iter().rev().take(500).cloned().collect();
        let id = self.identity();
        json!({
            "rules": with, "states": out, "events": events,
            "firing": count("firing"), "pending": count("pending"), "no_data": count("no_data"),
            "routes": self.routes_json(), "file": self.file.as_ref().map(|p| p.display().to_string()), "webhook": self.webhook(),
            "coordinator": self.is_coordinator(), "cluster": {"name": id.name, "public_url": id.public_url},
        })
    }

    /// One rule's state, with its events in memory, the newest first.
    pub fn one(&self, id: &str) -> Result<Json, ApiError> {
        let r = self.rule(id)?;
        let st = lock(&self.states).get(id).cloned();
        let mut j = self.state_json(&r, st.as_ref());
        let events: Vec<Arc<Event>> = lock(&self.events).iter().rev().filter(|e| e.rule == id).cloned().collect();
        j["events"] = json!(events);
        j["rule_json"] = json!(r);
        Ok(j)
    }

    pub fn routes_json(&self) -> Vec<Json> {
        let d = lock(&self.deliveries).clone();
        self.routes()
            .iter()
            .map(|r| {
                let mut j = json!({"name": r.name, "url": r.shown_url(), "severities": r.severities});
                let s = d.get(&r.name).cloned().unwrap_or_default();
                for (k, v) in json!(s).as_object().into_iter().flatten() {
                    j[k] = v.clone();
                }
                j
            })
            .collect()
    }

    /// What a copy says of its alerts in its detail, for a coordinator
    /// (`detail.alerts`): the alerts firing, how many are pending and
    /// without data, and each each_copy rule's state with its rule's
    /// digest.
    pub fn summary(&self) -> Json {
        let rules = self.rules();
        let states = lock(&self.states);
        let mut firing = Vec::new();
        let (mut pending, mut no_data) = (0, 0);
        let mut each = serde_json::Map::new();
        for r in rules.iter().filter(|r| r.enabled) {
            let Some(s) = states.get(&r.id) else { continue };
            match s.state {
                "firing" => firing.push(json!({"id": r.id, "name": r.name, "severity": r.severity, "since_ms": s.since_ms, "value": s.value})),
                "pending" => pending += 1,
                "no_data" => no_data += 1,
                _ => {}
            }
            if r.copies() && !self.is_coordinator() {
                each.insert(r.id.clone(), json!({"state": s.state, "value": s.value, "since_ms": s.since_ms, "sig": r.sig()}));
            }
        }
        json!({"firing": firing, "pending": pending, "no_data": no_data, "rules": each})
    }

    /// Alerts firing, by severity, for `/metrics`.
    pub fn firing(&self) -> Vec<(String, f64)> {
        let rules = self.rules();
        let states = lock(&self.states);
        ["critical", "warning", "info"]
            .iter()
            .map(|sev| {
                let n = rules.iter().filter(|r| r.severity == *sev && states.get(&r.id).is_some_and(|s| s.state == "firing")).count();
                (format!("severity=\"{sev}\""), n as f64)
            })
            .collect()
    }

    /// The text a notification of `e` says.
    pub fn message(&self, e: &Event) -> String {
        let id = self.identity();
        let place = match e.where_.len() {
            0 => String::new(),
            1..=3 => format!("/{}", e.where_.join(",")),
            n => format!("/{} and {} more", e.where_[..3].join(","), n - 3),
        };
        let value = e.value.map_or("no data".to_string(), number);
        let window = duration(e.window_s * 1000);
        let mut text = match e.state {
            "resolved" => {
                let after = e.fired_ms.map(|f| format!(" after {}", duration(e.ts_ms.saturating_sub(f)))).unwrap_or_default();
                let why = e.why.as_ref().map(|w| format!(" ({w}{})", e.by.as_ref().map(|b| format!(" by {b}")).unwrap_or_default())).unwrap_or_default();
                format!("[strata RESOLVED {}] {} on {}{place}: {} = {value}{after}{why}.", e.severity, e.name, id.name, e.metric)
            }
            state => format!(
                "[strata {} {}] {} on {}{place}: {} = {value} ({} {}, {} over {window}).",
                state.to_uppercase(),
                e.severity,
                e.name,
                id.name,
                e.metric,
                e.op,
                number(e.threshold),
                e.eval
            ),
        };
        if e.state == "firing" && !e.description.is_empty() {
            text.push(' ');
            text.push_str(&e.description);
        }
        if !e.runbook.is_empty() {
            text.push_str(&format!(" Runbook: {}", e.runbook));
        }
        if let Some(u) = &id.public_url {
            text.push_str(&format!(" {u}/#/alert/{}", e.rule));
        }
        text
    }

    /// Posts the events that notify to the routes that take them, trying
    /// each three times, and counts what was delivered.
    pub async fn notify(&self, changes: &[Arc<Event>]) {
        for e in changes.iter().filter(|e| e.notifies()) {
            let routes: Vec<Route> = self.routes().into_iter().filter(|r| if e.route.is_empty() { r.takes(&e.severity) } else { r.name == e.route }).collect();
            let text = self.message(e);
            for r in routes {
                let _ = self.post(&r, &json!({"text": text, "alert": &**e})).await;
            }
        }
    }

    /// Posts `body` to route `r`, three times at most; counts how it went.
    async fn post(&self, r: &Route, body: &Json) -> Result<(), String> {
        let mut last = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << attempt))).await;
            }
            match self.http.post(&r.url).json(body).send().await.and_then(|r| r.error_for_status()) {
                Ok(_) => {
                    let mut d = lock(&self.deliveries);
                    let d = d.entry(r.name.clone()).or_default();
                    d.sent += 1;
                    d.last_ok_ms = Some(now_ms());
                    return Ok(());
                }
                Err(e) => last = e.without_url().to_string(),
            }
        }
        tracing::warn!(route = %r.name, error = %last, "could not post an alert to its route");
        let mut d = lock(&self.deliveries);
        let d = d.entry(r.name.clone()).or_default();
        d.failed += 1;
        d.last_error = Some(last.clone());
        d.last_error_ms = Some(now_ms());
        Err(last)
    }

    /// Posts a test message to route `name`.
    pub async fn test_route(&self, name: &str, by: &str) -> Result<Json, ApiError> {
        let r = self.routes().into_iter().find(|r| r.name == name).ok_or_else(|| ApiError { status: 404, message: format!("no route {name}") })?;
        let id = self.identity();
        let text = format!("[strata TEST] A test of route {} on {}, sent by {by}.{}", r.name, id.name, id.public_url.map(|u| format!(" {u}/#/alerts")).unwrap_or_default());
        let t0 = std::time::Instant::now();
        match self.post(&r, &json!({"text": text, "test": true})).await {
            Ok(()) => Ok(json!({"route": r.name, "ok": true, "ms": t0.elapsed().as_secs_f64() * 1e3, "text": text})),
            Err(e) => Ok(json!({"route": r.name, "ok": false, "error": e, "ms": t0.elapsed().as_secs_f64() * 1e3, "text": text})),
        }
    }

    /// Replays `r` over `points` (each with `t` and the rule's metric, the
    /// oldest first): its window's value at each point and when it fired.
    pub fn backtest(&self, r: &Rule, points: &[Arc<Json>]) -> Json {
        let mut st = St::new(points.first().and_then(|p| p["t"].as_u64()).unwrap_or(0));
        let mut fired: Vec<Json> = Vec::new();
        let mut open: Option<u64> = None;
        let mut out = Vec::with_capacity(points.len());
        for (i, p) in points.iter().enumerate() {
            let t = p["t"].as_u64().unwrap_or(0);
            // The points up to this one.
            let v = window_value(r, &points[..=i], t);
            out.push(json!({"t": t, "value": p[&r.metric], "window": v, "state": st.state}));
            match st.step(r, v, t) {
                Some("firing") => open = Some(t),
                Some(_) => {
                    if let Some(f) = open.take() {
                        fired.push(json!({"from_ms": f, "to_ms": t}));
                    }
                }
                None => {}
            }
            if let Some(last) = out.last_mut() {
                last["state"] = json!(st.state);
            }
        }
        if let Some(f) = open {
            fired.push(json!({"from_ms": f, "to_ms": Json::Null}));
        }
        json!({"points": out, "fired": fired})
    }
}

fn push_note(st: &mut St, by: &str, t: u64, text: String) {
    if st.notes.len() == NOTES {
        st.notes.remove(0);
    }
    st.notes.push(Note { by: by.into(), at_ms: t, text });
}

/// Whether a copy in `/v1/cluster` answers.
fn answers(r: &Json) -> bool {
    r["status"] != "down" && r["role"] != "unknown"
}

/// The value of rule `r`'s window ending at `t` (from `t - window_s` to
/// `t`) over `points` (the oldest first): `None` when no point of the
/// window has the metric, and while the series spans under half the
/// window (a server just started). The mean of `error_ratio` and
/// `availability` weighs each point by its queries.
pub fn window_value(r: &Rule, points: &[Arc<Json>], t: u64) -> Option<f64> {
    let w = r.window_s * 1000;
    let from = t.saturating_sub(w);
    let at = |p: &Json| p["t"].as_u64().unwrap_or(0);
    let start = points.partition_point(|p| at(p) < from);
    let window: Vec<&Arc<Json>> = points[start..].iter().filter(|p| at(p) <= t).collect();
    let first = at(window.first()?);
    if t.saturating_sub(first) < (w / 2).saturating_sub(crate::telemetry::INTERVAL.as_millis() as u64) {
        return None;
    }
    if r.eval == "mean" && matches!(r.metric.as_str(), "error_ratio" | "availability") && window.iter().any(|p| p.get("queries").is_some()) {
        let (mut q, mut e) = (0.0, 0.0);
        for p in &window {
            q += p["queries"].as_f64().unwrap_or(0.0);
            e += p["query_errors"].as_f64().unwrap_or(0.0);
        }
        if q == 0.0 {
            return None;
        }
        let ratio = e / q;
        return Some(if r.metric == "availability" { 1.0 - ratio } else { ratio });
    }
    let vals: Vec<(u64, f64)> = window.iter().filter_map(|p| p.get(&r.metric).and_then(Json::as_f64).map(|v| (at(p), v))).collect();
    aggregate(r, &vals)
}

/// One value of a window's `(time, value)` pairs, by the rule's `eval`.
fn aggregate(r: &Rule, vals: &[(u64, f64)]) -> Option<f64> {
    let (first, last) = (vals.first()?, vals.last()?);
    let it = || vals.iter().map(|(_, v)| *v);
    Some(match r.eval.as_str() {
        "max" => it().fold(f64::MIN, f64::max),
        "min" => it().fold(f64::MAX, f64::min),
        "rate" => {
            let secs = (last.0.saturating_sub(first.0)) as f64 / 1e3;
            if secs <= 0.0 {
                return None;
            }
            (last.1 - first.1) / secs
        }
        // Every point meets the condition when the least extreme does.
        "all" if r.op.starts_with('>') => it().fold(f64::MAX, f64::min),
        "all" => it().fold(f64::MIN, f64::max),
        _ => it().sum::<f64>() / vals.len() as f64,
    })
}

/// A number as messages show it: up to four significant digits.
fn number(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let digits = (3 - v.abs().log10().floor() as i32).clamp(0, 6) as usize;
    let s = format!("{v:.digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// A duration as messages show it: `45 s`, `5 min`, `2 h 10 min`.
fn duration(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=119 => format!("{s} s"),
        120..=7199 => format!("{} min", s / 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// Evaluates the alerts after a new point of the series: this server's
/// rules now, and on a coordinator its cluster at most every 5 seconds,
/// in a task of its own. Changes go to the events, the log store and the
/// routes.
pub fn tick(svc: &Arc<crate::service::Service>, point: &Json) {
    let coordinator = matches!(svc.backend, crate::service::Backend::Cluster(_));
    svc.alerts.set_coordinator(coordinator);
    let t = point["t"].as_u64().unwrap_or_else(now_ms);
    let points = svc.series.since(t.saturating_sub(MAX_WINDOW_S * 1000));
    let changes = svc.alerts.check(&points, t);
    deliver(svc, changes);
    let now = now_ms();
    if coordinator && now.saturating_sub(svc.alerts.cluster_checked.load(Ordering::Relaxed)) >= 5_000 && !svc.alerts.cluster_running.swap(true, Ordering::AcqRel) {
        let svc = svc.clone();
        tokio::spawn(async move {
            check_cluster(&svc).await;
            svc.alerts.cluster_running.store(false, Ordering::Release);
        });
    }
}

/// A coordinator's check of its cluster now ([`Alerts::check_cluster`]).
pub async fn check_cluster(svc: &Arc<crate::service::Service>) {
    let snap = svc.cluster().await;
    let t = now_ms();
    svc.alerts.cluster_checked.store(t, Ordering::Relaxed);
    let changes = svc.alerts.check_cluster(&snap, t);
    deliver(svc, changes);
}

/// Days the log store keeps alert events (its table `alerts`).
pub const KEEP_DAYS: u64 = 7;

/// Hands alert events to the server's events and log store, and posts
/// those that notify.
pub fn deliver(svc: &Arc<crate::service::Service>, changes: Vec<Arc<Event>>) {
    if changes.is_empty() {
        return;
    }
    svc.alerts.set_identity(svc.identity.name.clone(), svc.identity.public_url.clone());
    for e in &changes {
        svc.events.add(event_of(e));
        if let Some(store) = &svc.log_store {
            store.keep_alert(e);
        }
    }
    if changes.iter().any(|e| e.notifies()) {
        let svc = svc.clone();
        tokio::spawn(async move { svc.alerts.notify(&changes).await });
    }
}

/// An alert's change as one of the server's events ([`crate::events`]):
/// `alert_fired`, `alert_resolved`, `alert_acked`, `alert_silenced` (a
/// silence lifted too), or a `note`; where: the rule, and the copy or
/// shard when one is named.
pub fn event_of(e: &Event) -> crate::events::Event {
    let kind = match e.state {
        "firing" => "alert_fired",
        "resolved" => "alert_resolved",
        "acked" => "alert_acked",
        "silenced" | "unsilenced" => "alert_silenced",
        _ => "note",
    };
    let severity = if e.state == "firing" { e.severity.clone() } else { "info".to_string() };
    let by = e.by.clone().unwrap_or_else(|| "someone".into());
    let place = if e.where_.is_empty() { String::new() } else { format!(" on {}", e.where_.join(", ")) };
    let value = e.value.map_or_else(|| "no data".to_string(), number);
    let text = e.text.as_ref().map(|t| format!(": {t}")).unwrap_or_default();
    let title = match e.state {
        "firing" => format!("{} fires{place}: {} = {value} ({} {}, {} over {})", e.name, e.metric, e.op, number(e.threshold), e.eval, duration(e.window_s * 1000)),
        "resolved" => {
            let after = e.fired_ms.map(|f| format!(" after {}", duration(e.ts_ms.saturating_sub(f)))).unwrap_or_default();
            let why = e.why.as_ref().map(|w| format!(" ({w}{})", e.by.as_ref().map(|b| format!(" by {b}")).unwrap_or_default())).unwrap_or_default();
            format!("{} resolved{after}: {} = {value}{why}", e.name, e.metric)
        }
        "acked" => format!("{by} acknowledged {}{text}", e.name),
        "silenced" => format!("{by} silenced {} {}", e.name, e.text.as_deref().unwrap_or_default()),
        "unsilenced" => format!("{by} lifted the silence of {}", e.name),
        _ => format!("{by} on {}{text}", e.name),
    };
    let mut where_ = json!({"rule": e.rule});
    if let Some(w) = e.place.first() {
        for k in ["shard", "copy"] {
            if !w[k].is_null() {
                where_[k] = w[k].clone();
            }
        }
    }
    crate::events::Event { kind: kind.into(), severity, title, detail: serde_json::to_value(e).unwrap_or(Json::Null), where_, who: e.by.clone() }
}

/// The rules an edit changed, before and after, by id (`null` where it
/// did not exist), for the audit entry.
pub fn changed(before: &[Rule], after: &[Rule]) -> (Json, Json) {
    let (mut b, mut a) = (serde_json::Map::new(), serde_json::Map::new());
    let ids: Vec<&String> = before.iter().map(|r| &r.id).chain(after.iter().map(|r| &r.id)).collect();
    for id in ids {
        let (x, y) = (before.iter().find(|r| &r.id == id), after.iter().find(|r| &r.id == id));
        if x != y && !b.contains_key(id) {
            b.insert(id.clone(), json!(x));
            a.insert(id.clone(), json!(y));
        }
    }
    (Json::Object(b), Json::Object(a))
}

/// What [`changed`] says, in words: "added x; edited y; removed z".
pub fn change_text(before: &Json, after: &Json) -> String {
    let ids = |f: &dyn Fn(&Json, &Json) -> bool| -> Vec<String> { before.as_object().into_iter().flatten().filter(|(k, b)| f(b, &after[k.as_str()])).map(|(k, _)| k.clone()).collect() };
    let parts: Vec<String> = [("added", ids(&|b, a| b.is_null() && !a.is_null())), ("edited", ids(&|b, a| !b.is_null() && !a.is_null())), ("removed", ids(&|b, a| !b.is_null() && a.is_null()))]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(w, v)| format!("{w} {}", v.join(", ")))
        .collect();
    if parts.is_empty() {
        "no rule changed".into()
    } else {
        parts.join("; ")
    }
}

/// The metrics the log store's `load` table keeps, a row a minute: a
/// backtest past memory's hour replays only these.
pub const LOAD_METRICS: [&str; 14] = ["qps", "cpu", "busy", "p99_ms", "p50_ms", "disk_used_ratio", "error_ratio", "availability", "errors_s", "client_errors_s", "timeouts_s", "rejected_s", "writes_s", "mem_bytes"];

/// Replays a rule (`{"rule", "from", "to"}`, 7 days at most) over this
/// server's points: memory's when it holds `from`, else a point a minute
/// from the log store's `load` table.
pub async fn backtest(svc: &Arc<crate::service::Service>, b: &Json) -> Result<Json, ApiError> {
    let mut r = b.get("rule").cloned().filter(Json::is_object).ok_or_else(|| ApiError::bad("the body needs \"rule\""))?;
    for k in ["id", "name"] {
        if r[k].is_null() {
            r[k] = json!("backtest");
        }
    }
    let rule: Rule = serde_json::from_value(r).map_err(|e| ApiError::bad(format!("bad rule: {e}")))?;
    rule.check()?;
    let now = now_ms();
    let to = b["to"].as_u64().unwrap_or(now).min(now);
    let from = b["from"].as_u64().unwrap_or(to.saturating_sub(86_400_000)).max(to.saturating_sub(KEEP_DAYS * 86_400_000));
    if from >= to {
        return Err(ApiError::bad("from comes before to"));
    }
    let lead = from.saturating_sub(rule.window_s * 1000 + rule.for_s * 1000);
    let memory = svc.series.since(0);
    let held = memory.first().and_then(|p| p["t"].as_u64());
    let (points, source) = match held {
        Some(h) if h <= from => (memory.into_iter().filter(|p| p["t"].as_u64().is_some_and(|t| t >= lead && t <= to)).collect::<Vec<_>>(), "memory"),
        _ => {
            let store = svc.log_store.clone().ok_or_else(|| ApiError { status: 409, message: "memory holds the last hour, and this server keeps no log store for older points".into() })?;
            if !LOAD_METRICS.contains(&rule.metric.as_str()) {
                return Err(ApiError::bad(format!("the log store's load table keeps {} a minute, not {}: a backtest past the last hour replays only those", LOAD_METRICS.join(", "), rule.metric)));
            }
            let col = if rule.metric == "availability" { "error_ratio" } else { rule.metric.as_str() };
            let mut points = Vec::new();
            // A day a query: a day's minutes are under the store's rows.
            let mut lo = lead;
            while lo < to {
                let hi = (lo + 86_400_000).min(to + 1);
                let params = strata::query::Params::new().set("lo", strata::value::Value::Timestamp(lo as i64 * 1000)).set("hi", strata::value::Value::Timestamp(hi as i64 * 1000));
                let sql = format!("SELECT ts, {col} FROM load WHERE ts >= :lo AND ts < :hi ORDER BY ts LIMIT 5000");
                let (rs, _) = store.query(&sql, &params, false, std::time::Duration::from_secs(10)).await?;
                for row in &rs.rows {
                    let (Some(strata::value::Value::Timestamp(ts)), Some(v)) = (row.first(), row.get(1).and_then(|v| v.as_f64())) else { continue };
                    let v = if rule.metric == "availability" { 1.0 - v } else { v };
                    let mut p = json!({"t": (*ts / 1000) as u64});
                    p[rule.metric.as_str()] = json!(v);
                    points.push(Arc::new(p));
                }
                lo = hi;
            }
            (points, "load")
        }
    };
    let mut j = svc.alerts.backtest(&rule, &points);
    if let Some(ps) = j["points"].as_array_mut() {
        ps.retain(|p| p["t"].as_u64().is_some_and(|t| t >= from));
    }
    j["source"] = json!(source);
    j["from"] = json!(from);
    j["to"] = json!(to);
    j["metric"] = json!(rule.metric);
    Ok(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(vals: &[(u64, f64)]) -> Vec<Arc<Json>> {
        vals.iter().map(|(t, v)| Arc::new(json!({"t": t, "p99_ms": v}))).collect()
    }

    fn slow(eval: &str, window_s: u64, for_s: u64) -> Rule {
        Rule { eval: eval.into(), window_s, for_s, ..rule("p99", "slow", "p99_ms", ">", 100.0, "mean", 300, 0, "warning", "self", "") }
    }

    #[test]
    fn an_alert_fires_after_its_condition_held_and_resolves() {
        let a = Alerts::new(None, None).unwrap();
        a.set_rules(vec![slow("max", 2, 10)], "test").unwrap();
        let mut ps = Vec::new();
        let mut run = |t: u64, v: Option<f64>| {
            ps.push(Arc::new(match v {
                Some(v) => json!({"t": t, "p99_ms": v}),
                None => json!({"t": t}),
            }));
            a.check(&ps, t)
        };
        assert!(run(0, Some(50.0)).is_empty());
        assert!(run(2_000, Some(150.0)).is_empty());
        assert_eq!(a.json()["states"][0]["state"], "pending");
        assert!(run(8_000, Some(150.0)).is_empty());
        let fired = run(12_000, Some(150.0));
        assert_eq!((fired.len(), fired[0].state), (1, "firing"));
        assert_eq!(a.json()["firing"], 1);
        assert_eq!(a.firing()[1], ("severity=\"warning\"".to_string(), 1.0));
        // No data keeps it firing, however long; then it resolves.
        assert!(run(20_000, None).is_empty());
        assert_eq!(a.json()["states"][0]["state"], "firing");
        assert_eq!(a.json()["states"][0]["no_data_since_ms"], 20_000);
        let resolved = run(24_000, Some(10.0));
        assert_eq!(resolved[0].state, "resolved");
        assert_eq!(resolved[0].fired_ms, Some(12_000));
        assert_eq!(a.json()["events"].as_array().unwrap().len(), 2);
        assert_eq!(a.json()["events"][0]["state"], "resolved");
    }

    #[test]
    fn a_window_s_mean_fires_where_every_point_does_not() {
        // Above the threshold on average, with a dip every third point.
        let vals: Vec<(u64, f64)> = (0..120u64).map(|i| (i * 2000, if i % 3 == 2 { 20.0 } else { 160.0 })).collect();
        let ps = points(&vals);
        let mean = slow("mean", 60, 0);
        let every = slow("all", 60, 0);
        let (m, e) = (Alerts::new(None, None).unwrap(), Alerts::new(None, None).unwrap());
        m.set_rules(vec![mean], "t").unwrap();
        e.set_rules(vec![every], "t").unwrap();
        let (mut mf, mut ef) = (0, 0);
        for i in 0..ps.len() {
            let t = ps[i]["t"].as_u64().unwrap();
            mf += m.check(&ps[..=i], t).iter().filter(|e| e.state == "firing").count();
            ef += e.check(&ps[..=i], t).iter().filter(|e| e.state == "firing").count();
        }
        assert_eq!((mf, ef), (1, 0), "the mean fires once and stays; every point never holds");
    }

    #[test]
    fn resolve_holds_a_firing_alert_until_it_is_passed() {
        let a = Alerts::new(None, None).unwrap();
        a.set_rules(vec![Rule { resolve: Some(80.0), ..slow("max", 1, 0) }], "t").unwrap();
        let mut ps = Vec::new();
        let mut run = |t: u64, v: f64| {
            ps.push(Arc::new(json!({"t": t, "p99_ms": v})));
            a.check(&ps, t).iter().map(|e| e.state).collect::<Vec<_>>()
        };
        assert_eq!(run(0, 120.0), ["firing"]);
        assert!(run(2000, 90.0).is_empty(), "under the threshold, over resolve: still firing");
        assert_eq!(run(4000, 70.0), ["resolved"]);
        let bad = Rule { resolve: Some(120.0), ..slow("max", 1, 0) };
        assert!(Alerts::new(None, None).unwrap().set_rules(vec![bad], "t").is_err());
    }

    #[test]
    fn editing_a_firing_rule_resolves_it_and_says_why() {
        let a = Alerts::new(None, None).unwrap();
        a.set_rules(vec![slow("max", 1, 0)], "t").unwrap();
        let ps = points(&[(0, 500.0)]);
        assert_eq!(a.check(&ps, 0)[0].state, "firing");
        let (resolved, before, after) = a.set_rules(vec![Rule { threshold: 1000.0, ..slow("max", 1, 0) }], "ana").unwrap();
        assert_eq!(resolved[0].state, "resolved");
        assert_eq!(resolved[0].why.as_deref(), Some("the rule was edited"));
        assert_eq!((before[0].threshold, after[0].threshold), (100.0, 1000.0));
        let (resolved, ..) = a.set_rules(vec![], "ana").unwrap();
        assert!(resolved.is_empty(), "it no longer fired");
    }

    #[test]
    fn rules_and_routes_are_checked_and_kept_in_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alerts.json");
        let a = Alerts::new(Some(path.clone()), Some("http://hooks.example/abc".into())).unwrap();
        assert_eq!(a.rules(), defaults());
        assert_eq!(a.routes()[0].name, "default");
        assert_eq!(a.routes_json()[0]["url"], "http://hooks.example/...");
        let mut bad = defaults();
        bad[0].op = "!=".into();
        assert!(a.set_rules(bad, "t").is_err());
        let mut two = defaults();
        two[1].id = two[0].id.clone();
        assert!(a.set_rules(two, "t").is_err());
        let mut fewer = defaults();
        fewer.truncate(2);
        a.set_rules(fewer.clone(), "t").unwrap();
        a.set_routes(vec![Route { name: "default".into(), url: String::new(), severities: vec!["critical".into()] }, Route { name: "pager".into(), url: "https://pager.example/x".into(), severities: vec![] }]).unwrap();
        let again = Alerts::new(Some(path.clone()), None).unwrap();
        assert_eq!(again.rules(), fewer);
        assert_eq!(again.routes()[0].url, "http://hooks.example/abc", "a route given without its url keeps it");
        // An old file: a list of rules.
        std::fs::write(&path, serde_json::to_vec(&json!([{"id": "x", "name": "x", "metric": "qps", "op": ">", "threshold": 1}])).unwrap()).unwrap();
        let old = Alerts::new(Some(path), None).unwrap();
        assert_eq!((old.rules()[0].eval.as_str(), old.rules()[0].window_s), ("mean", 300));
        // A zero wait fires once the series spans half the window.
        let fired = a.check(&[Arc::new(json!({"t": 0, "failing": 1.0})), Arc::new(json!({"t": 16_000, "failing": 1.0}))], 16_000);
        assert_eq!(fired[0].rule, "failing");
    }

    #[test]
    fn a_coordinator_gathers_its_copies_and_checks_its_cluster() {
        let a = Alerts::new(None, None).unwrap();
        a.set_coordinator(true);
        assert!(a.rules().iter().any(|r| r.id == "shard-no-majority" && r.builtin));
        let disk = defaults().into_iter().find(|r| r.id == "disk-full").unwrap();
        let copy = |url: &str, state: &str, v: f64| json!({"url": url, "status": "ok", "role": "primary", "takes_writes": true, "detail": {"load": {"t": 1_000}, "alerts": {"rules": {"disk-full": {"state": state, "value": v, "since_ms": 500, "sig": disk.sig()}}}}});
        let snap = json!({"shards": [
            {"shard": 0, "replicas": [copy("http://a", "ok", 0.5)]},
            {"shard": 1, "replicas": [copy("http://b", "firing", 0.95), {"url": "http://c", "status": "down", "role": "replica"}]},
        ]});
        let changes = a.check_cluster(&snap, 1_000);
        let fired: Vec<&str> = changes.iter().filter(|e| e.state == "firing").map(|e| e.rule.as_str()).collect();
        assert!(fired.contains(&"disk-full") && fired.contains(&"shard-no-majority"), "{fired:?}");
        let j = a.one("disk-full").unwrap();
        assert_eq!(j["where"][0]["short"], "s1-0", "{j}");
        assert_eq!(j["copies"]["firing"], 1);
        let m = a.one("shard-no-majority").unwrap();
        assert_eq!(m["where"][0]["shard"], 1, "{m}");
        assert!(changes.iter().any(|e| e.rule == "shard-no-majority" && e.where_ == ["shard 1"]));
        // The copy comes back: the shard has its majority again.
        let back = json!({"shards": [
            {"shard": 0, "replicas": [copy("http://a", "ok", 0.5)]},
            {"shard": 1, "replicas": [copy("http://b", "ok", 0.5), copy("http://c", "ok", 0.5)]},
        ]});
        let changes = a.check_cluster(&back, 30_000);
        assert!(changes.iter().any(|e| e.rule == "shard-no-majority" && e.state == "resolved"), "{changes:?}");
        assert!(changes.iter().any(|e| e.rule == "disk-full" && e.state == "resolved"));
    }

    #[test]
    fn messages_name_the_cluster_the_place_and_the_runbook() {
        let a = Alerts::new(None, None).unwrap();
        a.set_identity(Some("prod/search".into()), Some("https://cc.example/".into()));
        let r = Rule { runbook: "https://wiki/disk".into(), ..defaults().into_iter().find(|r| r.id == "disk-full").unwrap() };
        let mut st = St::new(0);
        st.value = Some(0.934);
        st.where_ = vec![json!({"short": "s1-0"})];
        let e = Event::of(&r, "firing", &st, 10);
        assert_eq!(
            a.message(&e),
            "[strata FIRING warning] Disk almost full on prod/search/s1-0: disk_used_ratio = 0.934 (> 0.9, mean over 5 min). Under 10% of the copy's disk is left. Runbook: https://wiki/disk https://cc.example/#/alert/disk-full"
        );
        let mut r2 = Event::of(&r, "resolved", &st, 600_010);
        (r2.fired_ms, r2.why, r2.by) = (Some(10), Some("the rule was deleted".into()), Some("ana".into()));
        assert_eq!(a.message(&r2), "[strata RESOLVED warning] Disk almost full on prod/search/s1-0: disk_used_ratio = 0.934 after 10 min (the rule was deleted by ana). Runbook: https://wiki/disk https://cc.example/#/alert/disk-full");
    }
}
