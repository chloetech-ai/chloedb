//! Growing the cluster as the crawl's tables fill: when a shard holds
//! more than `split_rows` rows of them, the cluster gets one more shard
//! and its slots are evened out over the shards (the rows move with them,
//! while writes go on). The new shard comes from one of:
//!
//! - `local`: a strata-server the crawler starts on its own machine,
//!   beside the shards and the coordinator it started ([`Local`]);
//! - `servers`: the next of a list of servers started for it, empty;
//! - `kube`: the `StrataCluster` the coordinator runs in, given one more
//!   shard (its operator starts the copies and evens out the slots);
//! - `off`: nothing; the status says when a shard would split.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tokio::process::{Child, Command};

use crate::job::now_ms;
use crate::strata::{shard_rows, ShardRows, Strata};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "how", rename_all = "lowercase")]
pub enum How {
    Off,
    Local,
    Servers { servers: Vec<String> },
    Kube,
}

#[derive(Clone, Debug, Serialize)]
pub struct Policy {
    #[serde(flatten)]
    pub how: How,
    /// Rows of the crawl's tables a shard holds before the cluster grows
    /// by one (0: never).
    pub split_rows: u64,
    pub max_shards: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct GrowEvent {
    pub at_ms: u64,
    pub kind: String,
    pub text: String,
}

#[derive(Default, Serialize)]
struct GrowState {
    /// What Strata is: a coordinator, a server alone, or not answering.
    role: Option<String>,
    shards: Vec<ShardRows>,
    /// Slots moving between shards now.
    moving: bool,
    moves: Option<Value>,
    /// Why the cluster does not grow now, if it would.
    waiting: Option<String>,
    events: Vec<GrowEvent>,
    error: Option<String>,
    checked_ms: u64,
    /// Rows of the crawl's tables, over every shard.
    rows: u64,
    tables: Vec<String>,
    /// Servers of `--grow servers` tried already (added, or refused).
    #[serde(skip)]
    tried: Vec<String>,
}

pub struct Grower {
    pub policy: Policy,
    strata: Strata,
    local: Option<Arc<Local>>,
    st: Mutex<GrowState>,
    tables: Mutex<Vec<String>>,
}

impl Grower {
    pub fn new(policy: Policy, strata: Strata, local: Option<Arc<Local>>) -> Arc<Grower> {
        Arc::new(Grower { policy, strata, local, st: Mutex::new(GrowState::default()), tables: Mutex::new(Vec::new()) })
    }

    fn st(&self) -> std::sync::MutexGuard<'_, GrowState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn event(&self, kind: &str, text: String) {
        tracing::info!(kind, "{text}");
        let mut g = self.st();
        if g.events.len() >= 200 {
            g.events.remove(0);
        }
        g.events.push(GrowEvent { at_ms: now_ms(), kind: kind.into(), text });
    }

    /// The tables whose rows count.
    pub fn watch(&self, table: &str) {
        let mut t = self.tables.lock().unwrap_or_else(|e| e.into_inner());
        if !t.iter().any(|x| x == table) {
            t.push(table.to_string());
        }
    }

    pub fn status(&self) -> Value {
        let g = self.st();
        json!({
            "policy": self.policy,
            "role": g.role, "shards": g.shards, "moving": g.moving, "moves": g.moves,
            "waiting": g.waiting, "events": g.events, "error": g.error, "checked_ms": g.checked_ms,
            "rows": g.rows, "tables": g.tables,
            "local": self.local.as_ref().map(|l| l.status()),
        })
    }

    /// Looks at the cluster every few seconds, and grows it when a shard
    /// is past `split_rows`.
    pub async fn run(self: Arc<Self>) {
        let mut over_since: Option<Instant> = None;
        let mut quiet_until = Instant::now();
        loop {
            let r = self.check(&mut over_since, &mut quiet_until).await;
            {
                let mut g = self.st();
                g.checked_ms = now_ms();
                g.error = r.err();
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn check(&self, over_since: &mut Option<Instant>, quiet_until: &mut Instant) -> Result<(), String> {
        let tables = self.tables.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let health = self.strata.health().await.map_err(|e| e.to_string());
        let health = match health {
            Ok(h) => h,
            Err(e) => {
                self.st().role = None;
                return Err(e);
            }
        };
        let role = health["role"].as_str().unwrap_or("server").to_string();
        if role != "coordinator" {
            let mut g = self.st();
            g.role = Some(role);
            g.shards.clear();
            let rows: u64 = tables.iter().filter_map(|t| health["tables"][t].as_u64()).sum();
            g.rows = rows;
            g.tables = tables;
            g.waiting = Some("ChloeDB is one server, not a cluster: it cannot split. Start the crawler with --local, or point it at a coordinator".into());
            return Ok(());
        }
        let cluster = self.strata.cluster().await.map_err(|e| e.to_string())?.unwrap_or(Value::Null);
        let mut shards: Vec<ShardRows> = shard_rows(&cluster, "");
        for s in &mut shards {
            s.rows = 0;
        }
        for t in &tables {
            for (i, s) in shard_rows(&cluster, t).into_iter().enumerate() {
                if let Some(x) = shards.get_mut(i) {
                    x.rows += s.rows;
                }
            }
        }
        let moves = self.strata.get("/v1/admin/moves").await.ok();
        let moving = moves.as_ref().is_some_and(|m| m["running"] == true) || !cluster["moving"].is_null();
        let rows: u64 = shards.iter().map(|s| s.rows).sum();
        let fullest = shards.iter().map(|s| s.rows).max().unwrap_or(0);
        let n = shards.len();
        {
            let mut g = self.st();
            if g.moving && !moving {
                let per: Vec<String> = shards.iter().map(|s| format!("{}", s.rows)).collect();
                drop(g);
                self.event("rebalanced", format!("slots even over {n} shards; rows of the crawl now {}", per.join(" / ")));
                *quiet_until = Instant::now() + Duration::from_secs(8);
                g = self.st();
            }
            g.role = Some(role);
            g.shards = shards.clone();
            g.moving = moving;
            g.moves = moves.map(|m| json!({"state": m["state"], "kind": m["job"]["kind"], "done": m["done"], "of": m["of"], "rows_done": m["rows_done"], "rate_rows_s": m["rate_rows_s"], "running": m["running"]}));
            g.rows = rows;
            g.tables = tables;
            g.waiting = None;
        }
        let p = &self.policy;
        if p.split_rows == 0 || fullest <= p.split_rows {
            *over_since = None;
            return Ok(());
        }
        let why = if moving {
            Some("slots are moving".to_string())
        } else if n >= p.max_shards {
            Some(format!("the cluster has its most shards ({})", p.max_shards))
        } else if Instant::now() < *quiet_until {
            Some("the last rebalance just ended; the shards' counts catch up".into())
        } else if matches!(p.how, How::Off) {
            Some(format!("a shard holds {fullest} rows, past {}; growing is off (--grow)", p.split_rows))
        } else {
            None
        };
        if let Some(w) = why {
            self.st().waiting = Some(w);
            return Ok(());
        }
        // Past it on two looks in a row: one look may be a stale count.
        let since = *over_since.get_or_insert_with(Instant::now);
        if since.elapsed() < Duration::from_secs(3) {
            return Ok(());
        }
        *over_since = None;
        self.grow(n, fullest).await?;
        *quiet_until = Instant::now() + Duration::from_secs(5);
        Ok(())
    }

    async fn grow(&self, n: usize, fullest: u64) -> Result<(), String> {
        let why = format!("the fullest shard held {fullest} rows, past {}", self.policy.split_rows);
        match &self.policy.how {
            How::Off => Ok(()),
            How::Kube => {
                let k = self.strata.get("/v1/kube").await.map_err(|e| format!("reading the StrataCluster: {e}"))?;
                if k["managed"] == false {
                    return Err("--grow kube: the coordinator does not run on Kubernetes".into());
                }
                let shards = k["spec"]["shards"].as_u64().unwrap_or(n as u64);
                let body = json!({"spec": {"shards": shards + 1}});
                self.strata.post("/v1/admin/kube", &body).await.map_err(|e| format!("asking for shard {}: {e}", shards + 1))?;
                self.event("shard", format!("asked Kubernetes for shard {} ({why}); the operator starts it and moves slots to it", shards + 1));
                Ok(())
            }
            How::Servers { servers } if servers.is_empty() => Err("--grow servers: no server left to add".into()),
            How::Servers { servers } => {
                let next = {
                    let mut g = self.st();
                    let next = servers.iter().find(|s| !g.tried.iter().any(|u| u.trim_end_matches('/') == s.trim_end_matches('/'))).cloned();
                    match &next {
                        Some(s) => g.tried.push(s.clone()),
                        None => g.waiting = Some("every server of --grow servers was added".into()),
                    }
                    next
                };
                match next {
                    Some(next) => self.add(&next, &why).await,
                    None => Ok(()),
                }
            }
            How::Local => {
                let local = self.local.as_ref().ok_or("--grow local needs --local")?;
                let grpc = local.add_shard().await?;
                self.add(&grpc, &why).await
            }
        }
    }

    async fn add(&self, grpc: &str, why: &str) -> Result<(), String> {
        let j = self.strata.post("/v1/admin/shards", &json!({"servers": [grpc]})).await.map_err(|e| format!("adding {grpc} as a shard: {e}"))?;
        let i = j["shard"].as_u64().unwrap_or(0);
        self.event("shard", format!("shard {i} added ({grpc}): {why}"));
        self.strata.post("/v1/admin/rebalance", &json!({})).await.map_err(|e| format!("evening out the slots: {e}"))?;
        self.st().moving = true;
        self.event("rebalance", format!("moving slots to shard {i}, while the crawl writes"));
        Ok(())
    }
}

// ---------------------------------------------------------------- a cluster of its own

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
struct LocalShard {
    http: String,
    grpc: String,
    db: String,
}

/// A cluster on this machine: shards and a coordinator, each a
/// strata-server the crawler starts, with their databases and logs in
/// one directory (`cluster.json` lists the shards, so that the crawler
/// started again serves the same cluster).
pub struct Local {
    pub dir: PathBuf,
    pub bin: PathBuf,
    pub listen: String,
    pub port_base: u16,
    /// The crawler's own address and token, for the coordinator's apps.
    pub app: Option<(String, Option<String>)>,
    shards: Mutex<Vec<LocalShard>>,
    children: Mutex<Vec<(String, Child)>>,
}

impl Local {
    pub fn new(dir: PathBuf, bin: PathBuf, listen: String, port_base: u16, app: Option<(String, Option<String>)>) -> Arc<Local> {
        Arc::new(Local { dir, bin, listen, port_base, app, shards: Mutex::new(Vec::new()), children: Mutex::new(Vec::new()) })
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.listen)
    }

    fn status(&self) -> Value {
        let shards = self.shards.lock().unwrap_or_else(|e| e.into_inner()).clone();
        json!({"dir": self.dir, "coordinator": self.url(), "shards": shards})
    }

    fn save(&self) -> Result<(), String> {
        let shards = self.shards.lock().unwrap_or_else(|e| e.into_inner()).clone();
        std::fs::write(self.dir.join("cluster.json"), serde_json::to_vec_pretty(&json!({"shards": shards})).expect("json")).map_err(|e| format!("writing cluster.json: {e}"))
    }

    async fn spawn(&self, name: &str, args: &[String], env: &[(&str, String)]) -> Result<(), String> {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(format!("{name}.log"))).map_err(|e| format!("{name}.log: {e}"))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&self.bin);
        cmd.args(args).stdout(Stdio::from(log)).stderr(Stdio::from(err)).stdin(Stdio::null()).kill_on_drop(true);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().map_err(|e| format!("starting {} ({}): {e}", self.bin.display(), name))?;
        self.children.lock().unwrap_or_else(|e| e.into_inner()).push((name.to_string(), child));
        Ok(())
    }

    async fn wait_up(&self, http: &str, what: &str) -> Result<(), String> {
        let s = Strata::new(http, None);
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(60) {
            if s.health().await.is_ok() {
                return Ok(());
            }
            // A server that exited: say so rather than wait.
            let exited = {
                let mut kids = self.children.lock().unwrap_or_else(|e| e.into_inner());
                kids.iter_mut().find(|(n, _)| n == what).and_then(|(n, c)| c.try_wait().ok().flatten().map(|code| format!("{n} exited ({code}): see {}", self.dir.join(format!("{n}.log")).display())))
            };
            if let Some(e) = exited {
                return Err(e);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(format!("{what} did not answer in a minute at {http}"))
    }

    fn shard_args(&self, s: &LocalShard) -> Vec<String> {
        let addr = |u: &str| u.trim_start_matches("http://").to_string();
        vec!["--db".into(), s.db.clone(), "--seq-no".into(), "--listen".into(), addr(&s.http), "--grpc".into(), addr(&s.grpc)]
    }

    /// Starts the cluster: its shards as `cluster.json` lists them (one
    /// new shard the first time), then the coordinator over them. Its URL.
    pub async fn start(&self) -> Result<String, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let listed: Vec<LocalShard> = std::fs::read(self.dir.join("cluster.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|j| serde_json::from_value(j["shards"].clone()).ok()).unwrap_or_default();
        *self.shards.lock().unwrap_or_else(|e| e.into_inner()) = listed.clone();
        if listed.is_empty() {
            self.new_shard().await?;
        } else {
            for (i, s) in listed.iter().enumerate() {
                self.spawn(&format!("shard-{i}"), &self.shard_args(s), &[]).await?;
            }
            for (i, s) in listed.iter().enumerate() {
                self.wait_up(&s.http, &format!("shard-{i}")).await?;
            }
        }
        self.save()?;
        let grpcs: Vec<String> = self.shards.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|s| s.grpc.clone()).collect();
        let mut args = vec!["--shards".to_string(), grpcs.join(","), "--listen".into(), self.listen.clone(), "--grpc".into(), "off".into()];
        let mut env = Vec::new();
        if let Some((url, token)) = &self.app {
            args.push("--app".into());
            args.push(format!("crawler={url}"));
            if let Some(t) = token {
                env.push(("STRATA_APP_TOKEN", t.clone()));
            }
        }
        self.spawn("coordinator", &args, &env).await?;
        self.wait_up(&self.url(), "coordinator").await?;
        Ok(self.url())
    }

    /// Starts one more shard server (empty, numbering its writes), and
    /// lists it. Its gRPC URL, for the coordinator to add.
    async fn new_shard(&self) -> Result<String, String> {
        let i = self.shards.lock().unwrap_or_else(|e| e.into_inner()).len();
        let port = self.port_base as usize + 10 * i;
        if port + 1 > u16::MAX as usize {
            return Err("no port left for another shard".into());
        }
        let s = LocalShard {
            http: format!("http://127.0.0.1:{port}"),
            grpc: format!("http://127.0.0.1:{}", port + 1),
            db: self.dir.join(format!("shard-{i}.strata")).to_string_lossy().into_owned(),
        };
        self.spawn(&format!("shard-{i}"), &self.shard_args(&s), &[]).await?;
        self.wait_up(&s.http, &format!("shard-{i}")).await?;
        self.shards.lock().unwrap_or_else(|e| e.into_inner()).push(s.clone());
        self.save()?;
        Ok(s.grpc)
    }

    pub async fn add_shard(&self) -> Result<String, String> {
        self.new_shard().await
    }

    /// Stops every server it started: SIGTERM, then 10 s to exit.
    pub async fn stop(&self) {
        let mut kids = std::mem::take(&mut *self.children.lock().unwrap_or_else(|e| e.into_inner()));
        // The last started first: shards added during the crawl, the
        // coordinator, then the first shards.
        kids.reverse();
        for (_, c) in &kids {
            #[cfg(unix)]
            if let Some(pid) = c.id() {
                // SAFETY: a signal to a child this process started.
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            #[cfg(not(unix))]
            let _ = c;
        }
        for (n, c) in &mut kids {
            match tokio::time::timeout(Duration::from_secs(10), c.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    tracing::warn!("{n} did not stop in 10 s: killed");
                    let _ = c.kill().await;
                }
            }
        }
    }
}
