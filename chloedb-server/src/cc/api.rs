//! What `cc` asks the server: the reads each screen polls, and the changes
//! its keys send, on the routes the web command center uses, so a key may
//! do here what it may do there and the audit log reads the same.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{json, Value as Json};
use tokio::sync::Notify;

use super::app::Screen;

/// How long a read may take before the screen says it failed.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// A statement from the SQL screen may run longer.
const SQL_TIMEOUT: Duration = Duration::from_secs(300);

/// Where the calls go, and with which key.
#[derive(Clone)]
pub struct Api {
    pub base: String,
    key: Option<String>,
    client: reqwest::Client,
}

/// A call that did not answer 2xx: the server's words, or why it was not reached.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(s) => write!(f, "{} ({s})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl Api {
    pub fn new(base: &str, key: Option<String>, ca: Option<&[u8]>) -> Result<Api, String> {
        let mut b = crate::http_client().connect_timeout(Duration::from_secs(5));
        if let Some(pem) = ca {
            b = b.add_root_certificate(reqwest::Certificate::from_pem(pem).map_err(|e| format!("--ca: {e}"))?);
        }
        Ok(Api { base: base.trim_end_matches('/').to_string(), key, client: b.build().map_err(|e| e.to_string())? })
    }

    /// The same key and client, to another address (a hub's other connection).
    pub fn at(&self, base: &str) -> Api {
        Api { base: base.trim_end_matches('/').to_string(), ..self.clone() }
    }

    pub async fn get(&self, path: &str) -> Result<Json, Failure> {
        self.call(Method::GET, path, None, None, READ_TIMEOUT).await
    }

    /// One call; `reason` goes in `x-chloedb-reason`, which the audit log keeps.
    pub async fn call(&self, method: Method, path: &str, body: Option<&Json>, reason: Option<&str>, timeout: Duration) -> Result<Json, Failure> {
        let url = format!("{}{path}", self.base);
        let mut r = self.client.request(method, &url).header("x-chloedb-app", "chloedb-cc").timeout(timeout);
        if let Some(k) = &self.key {
            r = r.bearer_auth(k);
        }
        if let Some(why) = reason.map(str::trim).filter(|w| !w.is_empty()) {
            r = r.header("x-chloedb-reason", crate::mcp::encode_header(why));
        }
        if let Some(b) = body {
            r = r.json(b);
        }
        let res = r.send().await.map_err(|e| Failure { status: None, message: format!("{} cannot be reached: {}", self.base, root_cause(&e)) })?;
        let status = res.status().as_u16();
        let text = res.text().await.map_err(|e| Failure { status: Some(status), message: e.to_string() })?;
        let j = crate::admin_cli::answer_of(status, &text);
        if (200..300).contains(&status) {
            Ok(j)
        } else {
            Err(Failure { status: Some(status), message: j["error"].as_str().map_or_else(|| format!("HTTP {status}"), String::from) })
        }
    }
}

/// The innermost error of a failed request: "connection refused", not reqwest's wrapping.
fn root_cause(e: &(dyn std::error::Error + 'static)) -> String {
    let mut cur = e;
    while let Some(next) = cur.source() {
        cur = next;
    }
    cur.to_string()
}

/// Who the key is, from `/v1/whoami`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Who {
    /// The key's name, or "root token".
    pub name: String,
    pub scope: String,
    pub root: bool,
    pub cluster: Option<String>,
    pub environment: Option<String>,
    /// The server asks this key for a reason with every change.
    pub reason_required: bool,
    /// A key limited to tables: its grants in words ("logs_* write,
    /// ctx_incidents read").
    pub limited: Option<String>,
}

impl Who {
    pub fn of(j: &Json) -> Who {
        let root = j["root"] == true;
        let name = if root { "root token".to_string() } else { j["key"]["name"].as_str().or(j["name"].as_str()).unwrap_or("no key").to_string() };
        Who {
            name,
            scope: j["scope"].as_str().unwrap_or("read").to_string(),
            root,
            cluster: j["cluster"]["name"].as_str().filter(|s| !s.is_empty()).map(String::from),
            environment: j["cluster"]["environment"].as_str().filter(|s| !s.is_empty()).map(String::from),
            reason_required: j["may"]["reason_required"] == true,
            limited: j["tables"].as_object().map(|t| t.iter().map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or("?"))).collect::<Vec<_>>().join(", ")),
        }
    }

    /// Whether it reads the server's own log (`GET /v1/logs`): an admin
    /// of the server, or a key for operations, never a key limited to
    /// tables (MARCO-313).
    pub fn reads_server_log(&self) -> bool {
        self.root || (self.limited.is_none() && matches!(self.scope.as_str(), "admin" | "ops" | "propose"))
    }

    /// Whether the key's scope is `scope` or above it (read, write, admin).
    pub fn can(&self, scope: &str) -> bool {
        let rank = |s: &str| match s {
            "admin" => 3,
            "write" => 2,
            "ops" | "propose" | "read" => 1,
            _ => 0,
        };
        self.root || rank(&self.scope) >= rank(scope)
    }
}

/// The answers the screens draw, the last of each.
#[derive(Clone, Debug, Default)]
pub struct Data {
    pub cluster: Json,
    pub fleet: Json,
    pub alerts: Json,
    pub stats: Json,
    pub live: Json,
    pub logs: Json,
    pub tables: Json,
    pub events: Json,
    /// `GET /v1/tables`: the tables the key reads, for the Logs screen of a
    /// key that does not read the server's own log.
    pub log_tables: Json,
}

/// What the poller reads besides the header's: the screen on show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Want {
    pub screen: Screen,
    pub log_level: Option<String>,
    pub log_q: String,
}

/// The answer to the SQL screen's statement.
#[derive(Clone, Debug)]
pub struct SqlAnswer {
    pub sql: String,
    pub answer: Result<Json, String>,
    pub at_ms: u64,
}

/// How a note reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Good,
    Bad,
}

/// A line the status bar shows for a while: what a change did.
#[derive(Clone, Debug)]
pub struct Note {
    pub text: String,
    pub tone: Tone,
    pub at: Instant,
}

/// What the poller, the changes and the screen share.
#[derive(Debug)]
pub struct Shared {
    /// Where reads go now; `gen` moves on when it changes, so that an
    /// answer from the old address is dropped.
    pub base: String,
    pub gen: u64,
    pub want: Want,
    pub paused: bool,
    pub data: Data,
    pub read_at: Option<Instant>,
    /// The reads of the last round that failed, each with why.
    pub errors: Vec<String>,
    pub note: Option<Note>,
    pub who: Who,
    /// `GET /v1/admin/hub` of the hub, when `:ctx` asked for it.
    pub hub: Option<Result<Json, String>>,
    pub sql: Option<SqlAnswer>,
    pub sql_running: bool,
}

impl Shared {
    pub fn new(base: &str, who: Who) -> Shared {
        Shared {
            base: base.trim_end_matches('/').to_string(),
            gen: 0,
            want: Want::default(),
            paused: false,
            data: Data::default(),
            read_at: None,
            errors: Vec::new(),
            note: None,
            who,
            hub: None,
            sql: None,
            sql_running: false,
        }
    }

    pub fn say(&mut self, text: impl Into<String>, tone: Tone) {
        self.note = Some(Note { text: text.into(), tone, at: Instant::now() });
    }
}

pub fn lock(s: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// The routes a round reads: the header's and Home's always, and the
/// screen's, as key `who` may read them.
pub fn reads(want: &Want, who: &Who) -> Vec<(&'static str, String)> {
    let mut r = vec![("cluster", "/v1/cluster".to_string()), ("fleet", "/v1/health/fleet".to_string())];
    match want.screen {
        Screen::Home | Screen::Cluster | Screen::Sql => {}
        Screen::Alerts => r.push(("alerts", "/v1/alerts".into())),
        Screen::Queries => {
            r.push(("stats", "/v1/queries/stats?limit=100".into()));
            r.push(("live", "/v1/queries".into()));
        }
        // The server's own log is an admin's: another key's Logs are the
        // log tables it reads.
        Screen::Logs if !who.reads_server_log() => r.push(("log_tables", "/v1/tables".into())),
        Screen::Logs => {
            let mut p = "/v1/logs?limit=500".to_string();
            if let Some(l) = &want.log_level {
                p.push_str(&format!("&level={l}"));
            }
            if !want.log_q.trim().is_empty() {
                p.push_str(&format!("&q={}", crate::mcp::encode(want.log_q.trim())));
            }
            r.push(("logs", p));
        }
        Screen::Tables => r.push(("tables", "/v1/index/tables".into())),
        Screen::Timeline => r.push(("events", "/v1/events?limit=300".into())),
    }
    r
}

/// One round of reads: what the screen on show needs, into `shared`.
pub async fn read_round(api: &Api, shared: &Mutex<Shared>) {
    let (base, gen, want, who) = {
        let s = lock(shared);
        (s.base.clone(), s.gen, s.want.clone(), s.who.clone())
    };
    let api = api.at(&base);
    let paths = reads(&want, &who);
    let got = futures::future::join_all(paths.iter().map(|(_, p)| api.get(p))).await;
    let mut s = lock(shared);
    if s.gen == gen {
        s.errors.clear();
        for ((k, p), r) in paths.iter().zip(got) {
            match r {
                Ok(j) => *slot(&mut s.data, k) = j,
                Err(e) => s.errors.push(format!("{p}: {e}")),
            }
        }
        s.read_at = Some(Instant::now());
    }
}

/// Reads what the screen on show needs every `every`, or at once when woken.
pub async fn poll(api: Api, shared: Arc<Mutex<Shared>>, wake: Arc<Notify>, every: Duration) {
    loop {
        if !lock(&shared).paused {
            read_round(&api, &shared).await;
        }
        tokio::select! {
            _ = tokio::time::sleep(every) => {}
            _ = wake.notified() => {}
        }
    }
}

fn slot<'a>(d: &'a mut Data, k: &str) -> &'a mut Json {
    match k {
        "cluster" => &mut d.cluster,
        "fleet" => &mut d.fleet,
        "alerts" => &mut d.alerts,
        "stats" => &mut d.stats,
        "live" => &mut d.live,
        "logs" => &mut d.logs,
        "tables" => &mut d.tables,
        "log_tables" => &mut d.log_tables,
        _ => &mut d.events,
    }
}

/// A change, or a read, the screen asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// `POST /v1/admin/shards/{shard}/promote`: a handover by the copies' votes.
    Promote { shard: u64, url: String, id: String },
    /// `POST /v1/admin/plan/run/stop`: the plan stops after its step.
    StopPlan,
    Ack { rule: String, name: String },
    Silence { rule: String, name: String, minutes: u64 },
    Unsilence { rule: String, name: String },
    /// `DELETE /v1/queries/{id}`.
    Cancel { id: String },
    Sql { sql: String },
    /// `GET /v1/admin/hub` of the hub `root`.
    Hub { root: String },
    WhoAmI,
}

/// An act, and the reason the person gave for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub act: Act,
    pub reason: Option<String>,
}

fn enc(s: &str) -> String {
    crate::mcp::encode(s)
}

/// Runs a job and says what it did on the status bar; reads again after a change.
pub async fn run_job(api: Api, shared: Arc<Mutex<Shared>>, wake: Arc<Notify>, job: Job) {
    let base = lock(&shared).base.clone();
    let api = api.at(&base);
    let why = job.reason.as_deref();
    let post = |path: String, body: Json| {
        let api = api.clone();
        let why = why.map(String::from);
        async move { api.call(Method::POST, &path, Some(&body), why.as_deref(), READ_TIMEOUT).await }
    };
    let (text, tone) = match &job.act {
        Act::Promote { shard, url, id } => match post(format!("/v1/admin/shards/{shard}/promote"), json!({"url": url})).await {
            Ok(j) => (format!("{id} is shard {shard}'s primary{}", j["term"].as_u64().map(|t| format!(", term {t}")).unwrap_or_default()), Tone::Good),
            Err(e) => (format!("{id} did not become shard {shard}'s primary: {e}"), Tone::Bad),
        },
        Act::StopPlan => match post("/v1/admin/plan/run/stop".into(), json!({})).await {
            Ok(_) => ("The plan stops once its step ends: resume or discard it from the web's Cluster view".into(), Tone::Good),
            Err(e) => (format!("The plan did not stop: {e}"), Tone::Bad),
        },
        Act::Ack { rule, name } => match post(format!("/v1/alerts/{}/ack", enc(rule)), json!({})).await {
            Ok(_) => (format!("Acknowledged: {name}"), Tone::Good),
            Err(e) => (format!("{name} was not acknowledged: {e}"), Tone::Bad),
        },
        Act::Silence { rule, name, minutes } => match post(format!("/v1/alerts/{}/silence", enc(rule)), json!({"minutes": minutes, "reason": why.unwrap_or_default()})).await {
            Ok(_) => (format!("{name} is silenced for {minutes} min"), Tone::Good),
            Err(e) => (format!("{name} was not silenced: {e}"), Tone::Bad),
        },
        Act::Unsilence { rule, name } => match api.call(Method::DELETE, &format!("/v1/alerts/{}/silence", enc(rule)), None, why, READ_TIMEOUT).await {
            Ok(_) => (format!("{name} pages again"), Tone::Good),
            Err(e) => (format!("{name} is still silenced: {e}"), Tone::Bad),
        },
        Act::Cancel { id } => match api.call(Method::DELETE, &format!("/v1/queries/{}", enc(id)), None, why, READ_TIMEOUT).await {
            Ok(_) => (format!("Query {id} is cancelled"), Tone::Good),
            Err(e) => (format!("Query {id} was not cancelled: {e}"), Tone::Bad),
        },
        Act::Sql { sql } => {
            lock(&shared).sql_running = true;
            let r = api.call(Method::POST, "/v1/sql", Some(&json!({"sql": sql})), why, SQL_TIMEOUT).await;
            let mut s = lock(&shared);
            s.sql_running = false;
            s.sql = Some(SqlAnswer { sql: sql.clone(), answer: r.map_err(|e| e.to_string()), at_ms: super::model::now_ms() });
            return;
        }
        Act::Hub { root } => {
            let r = api.at(root).get("/v1/admin/hub").await.map_err(|e| match e.status {
                Some(404) => "this server is no hub: it has no /v1/admin/hub (a hub is chloedb-server 0.2 or later)".to_string(),
                Some(401 | 403) => format!("the key may not read the hub's connections, which takes an admin key: {e}"),
                _ => e.to_string(),
            });
            lock(&shared).hub = Some(r);
            return;
        }
        Act::WhoAmI => {
            let r = api.get("/v1/whoami").await;
            let mut s = lock(&shared);
            if lock_base_is(&s, &base) {
                match r {
                    Ok(j) => s.who = Who::of(&j),
                    Err(e) => s.say(format!("{base} does not take this key: {e}"), Tone::Bad),
                }
            }
            return;
        }
    };
    lock(&shared).say(text, tone);
    wake.notify_one();
}

fn lock_base_is(s: &Shared, base: &str) -> bool {
    s.base == base
}

/// The hub a URL goes through, and the connection it names:
/// `https://hub:7700/c/prod` is the hub `https://hub:7700` and `prod`.
pub fn hub_of(base: &str) -> (String, Option<String>) {
    let b = base.trim_end_matches('/');
    if let Some(i) = b.rfind("/c/") {
        let name = &b[i + 3..];
        if !name.is_empty() && !name.contains('/') {
            return (b[..i].to_string(), Some(name.to_string()));
        }
    }
    (b.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hub_url_names_its_connection() {
        assert_eq!(hub_of("https://hub:7700/c/prod/"), ("https://hub:7700".into(), Some("prod".into())));
        assert_eq!(hub_of("http://127.0.0.1:7700"), ("http://127.0.0.1:7700".into(), None));
        assert_eq!(hub_of("https://x/c/"), ("https://x/c".into(), None));
    }

    #[test]
    fn whoami_says_who_and_what_the_key_may() {
        let w = Who::of(&json!({"root": true, "scope": "admin", "cluster": {"name": "local-demo", "environment": null}, "may": {"reason_required": false}}));
        assert_eq!((w.name.as_str(), w.cluster.as_deref(), w.environment.as_deref()), ("root token", Some("local-demo"), None));
        assert!(w.can("admin"));
        let r = Who::of(&json!({"key": {"name": "dana"}, "scope": "read", "cluster": {"name": "prod", "environment": "production"}, "may": {"reason_required": true}}));
        assert_eq!(r.name, "dana");
        assert!(r.can("read") && !r.can("write") && r.reason_required);
    }

    #[test]
    fn each_screen_reads_its_own_routes() {
        let w = |screen| Want { screen, ..Want::default() };
        let admin = Who::of(&json!({"root": true, "scope": "admin"}));
        assert_eq!(reads(&w(Screen::Home), &admin).len(), 2);
        assert!(reads(&w(Screen::Queries), &admin).iter().any(|(k, p)| *k == "live" && p == "/v1/queries"));
        let logs = Want { screen: Screen::Logs, log_level: Some("warn".into()), log_q: "shard 0".into() };
        assert_eq!(reads(&logs, &admin)[2].1, "/v1/logs?limit=500&level=warn&q=shard%200");
        // MARCO-313: a key limited to tables does not read the server's own
        // log, but the log tables it reads.
        let oncall = Who::of(&json!({"key": {"name": "netops"}, "scope": "write", "tables": {"logs_*": "write", "ctx_incidents": "read"}}));
        assert_eq!(oncall.limited.as_deref(), Some("logs_* write, ctx_incidents read"));
        assert!(!oncall.reads_server_log() && admin.reads_server_log());
        assert_eq!(reads(&logs, &oncall)[2], ("log_tables", "/v1/tables".to_string()));
        let writer = Who::of(&json!({"key": {"name": "w"}, "scope": "write"}));
        assert!(!writer.reads_server_log() && Who::of(&json!({"key": {"name": "bot"}, "scope": "ops"})).reads_server_log());
    }
}
