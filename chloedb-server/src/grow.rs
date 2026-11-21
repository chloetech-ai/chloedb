//! `chloedb-server grow`: the database one server served becomes a cluster
//! on this machine, answering at the same address, in one command.
//!
//! The database becomes the first shard as it is (its rows stay where
//! they are), empty shards are started beside it with its tables, a
//! coordinator takes the server's address, and the slots are evened out
//! over the shards while the cluster answers: rows move to the new shards
//! in the background. Every server is a `chloedb-server` this process
//! starts and stops; their databases and logs are in one directory
//! (`<db>.cluster/` by default), whose `cluster.json` lists the shards, so
//! that `grow` again starts the same cluster, and `grow --shards N` with
//! more shards than it has adds the rest.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::{Child, Command};

/// What `grow` is given.
pub struct Options {
    /// The database the server served: the first shard.
    pub db: PathBuf,
    /// How many shards the cluster has once grown.
    pub shards: usize,
    /// The address the server listened on, which the coordinator takes.
    pub listen: String,
    /// Where the new shards' databases, the logs and `cluster.json` go.
    pub dir: PathBuf,
    /// The chloedb-server to start (this one).
    pub bin: PathBuf,
    /// The token the servers take (`--token`, `$CHLOEDB_TOKEN`), which
    /// they read from the environment they inherit.
    pub token: Option<String>,
}

/// A shard of the cluster: its database and addresses.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Shard {
    pub db: String,
    pub http: String,
    pub grpc: String,
}

/// The servers `grow` started. [`Cluster::stop`] stops them (SIGTERM, then
/// 10 s to exit before they are killed); dropping it kills them at once.
pub struct Cluster {
    children: Vec<(String, Child)>,
    pub url: String,
    pub shards: Vec<Shard>,
    /// Shards added this run, whose slots are moving to them.
    pub added: usize,
    token: Option<String>,
    me: Api,
    listed: Vec<Shard>,
}

/// Ctrl+C and SIGTERM, caught from when it is made: `grow` stops the
/// servers it started on either, also while it starts them.
pub struct Signals {
    #[cfg(unix)]
    int: tokio::signal::unix::Signal,
    #[cfg(unix)]
    term: tokio::signal::unix::Signal,
}

impl Signals {
    pub fn new() -> std::io::Result<Signals> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Signals { int: signal(SignalKind::interrupt())?, term: signal(SignalKind::terminate())? })
        }
        #[cfg(not(unix))]
        Ok(Signals {})
    }

    /// The next signal, as the exit code of a process it ends: 130 for
    /// Ctrl+C, 143 for SIGTERM.
    pub async fn next(&mut self) -> i32 {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.int.recv() => 130,
                _ = self.term.recv() => 143,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            130
        }
    }
}

/// The shards' addresses: from ten ports above the cluster's, ten apart
/// (HTTP, then gRPC on the next port).
pub fn shard_at(listen: &str, dir: &Path, i: usize, first_db: &Path) -> Result<Shard, String> {
    let port: u16 = listen.rsplit(':').next().and_then(|p| p.parse().ok()).ok_or_else(|| format!("--listen {listen}: no port"))?;
    let p = port as usize + 10 + 10 * i;
    if p + 1 > u16::MAX as usize {
        return Err(format!("no port left for shard {i} above {port}"));
    }
    let db = if i == 0 { first_db.to_path_buf() } else { dir.join(format!("shard-{i}.chloedb")) };
    Ok(Shard { db: db.to_string_lossy().into_owned(), http: format!("http://127.0.0.1:{p}"), grpc: format!("http://127.0.0.1:{}", p + 1) })
}

/// The shards `cluster.json` in `dir` lists (none the first time).
pub fn listed(dir: &Path) -> Vec<Shard> {
    std::fs::read(dir.join("cluster.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|j| serde_json::from_value(j["shards"].clone()).ok()).unwrap_or_default()
}

fn save(dir: &Path, shards: &[Shard]) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(&json!({"shards": shards, "note": "the shards of the cluster `chloedb-server grow` runs: start it again with `chloedb-server grow`"})).expect("json");
    std::fs::write(dir.join("cluster.json"), body).map_err(|e| format!("{}: {e}", dir.join("cluster.json").display()))
}

impl Cluster {
    /// The cluster `o` describes, nothing started yet: checks that no
    /// server answers at its address and reads the shards it has.
    pub async fn new(o: &Options) -> Result<Cluster, String> {
        std::fs::create_dir_all(&o.dir).map_err(|e| format!("{}: {e}", o.dir.display()))?;
        let http = crate::http_client().no_proxy().timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())?;
        let me = Api { http, token: o.token.clone() };
        let url = format!("http://{}", crate::ready::browsable(&o.listen));
        if me.get(&format!("{url}/v1/health")).await.is_ok() {
            return Err(format!("a server answers at {url} already: stop it (Ctrl+C where it runs), then grow its database into a cluster that answers there"));
        }
        let mut shards = listed(&o.dir);
        if shards.is_empty() {
            if !o.db.exists() {
                return Err(format!("{} does not exist: start `chloedb-server --db {}` first, or name the database with --db", o.db.display(), o.db.display()));
            }
            shards.push(shard_at(&o.listen, &o.dir, 0, &o.db)?);
        }
        Ok(Cluster { children: Vec::new(), url, shards: Vec::new(), added: 0, token: o.token.clone(), me, listed: shards })
    }

    /// Starts the cluster, growing it to `o.shards` shards, and returns
    /// once the coordinator answers (the slots may still be moving). The
    /// servers started before an error, or before the future is dropped,
    /// are left for [`Cluster::stop`].
    pub async fn grow(&mut self, o: &Options) -> Result<(), String> {
        let shards = self.listed.clone();
        let me = self.me.clone();
        let url = self.url.clone();
        for (i, s) in shards.iter().enumerate() {
            self.spawn(o, &format!("shard-{i}"), &shard_args(s)).await?;
        }
        for (i, s) in shards.iter().enumerate() {
            self.wait_up(&me, &s.http, &o.dir, &format!("shard-{i}")).await?;
        }
        save(&o.dir, &shards)?;
        self.shards = shards.clone();
        // The coordinator over the shards the cluster has; the shards it
        // grows by are added through it, so that their slots move with
        // their rows.
        let grpcs: Vec<String> = shards.iter().map(|s| s.grpc.clone()).collect();
        let args = vec!["--shards".to_string(), grpcs.join(","), "--listen".into(), o.listen.clone(), "--grpc".into(), "off".into()];
        self.spawn_coordinator(o, &args).await?;
        self.wait_up(&me, &url, &o.dir, "coordinator").await?;
        // The tables of the first shard, for the shards to come. A new
        // shard's database starts with the cluster's first table (its
        // --schema), which the coordinator compares when it joins; the
        // other tables are made over SQL once it answers.
        let tables = tables_of(&me, &shards[0].http).await?;
        let first = me.get(&format!("{}/v1/schema", shards[0].http)).await?;
        let first_name = first.get("table").and_then(Value::as_str).unwrap_or_default().to_string();
        let first_file = o.dir.join("first-table.json");
        if let Some(schema) = first.get("schema") {
            std::fs::write(&first_file, schema.to_string()).map_err(|e| format!("{}: {e}", first_file.display()))?;
        }
        while self.shards.len() < o.shards {
            let i = self.shards.len();
            let s = shard_at(&o.listen, &o.dir, i, &o.db)?;
            // A database a grow that stopped part way made, which never
            // joined (cluster.json does not list it): set aside, not used.
            set_aside(&o.dir, Path::new(&s.db))?;
            let mut args = shard_args(&s);
            if !Path::new(&s.db).exists() && first_file.exists() && !chloedb::query::hint::internal_table(&first_name) {
                args.extend(["--schema".to_string(), first_file.display().to_string()]);
            }
            self.spawn(o, &format!("shard-{i}"), &args).await?;
            self.wait_up(&me, &s.http, &o.dir, &format!("shard-{i}")).await?;
            for (name, sql) in &tables {
                me.post(&format!("{}/v1/sql", s.http), &json!({"sql": sql})).await.map_err(|e| format!("making table {name} on shard {i}: {e}"))?;
            }
            me.post(&format!("{url}/v1/admin/shards"), &json!({"servers": [s.grpc]})).await.map_err(|e| format!("adding shard {i} ({}) to the cluster: {e}", s.grpc))?;
            self.shards.push(s);
            self.added += 1;
            save(&o.dir, &self.shards)?;
        }
        if self.added > 0 {
            me.post(&format!("{url}/v1/admin/rebalance"), &json!({})).await.map_err(|e| format!("evening out the slots over the shards: {e}"))?;
        }
        Ok(())
    }

    async fn spawn(&mut self, o: &Options, name: &str, args: &[String]) -> Result<(), String> {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(o.dir.join(format!("{name}.log"))).map_err(|e| format!("{name}.log: {e}"))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&o.bin);
        cmd.args(args).stdout(Stdio::from(log)).stderr(Stdio::from(err)).stdin(Stdio::null()).kill_on_drop(true);
        // The coordinator runs the apps; a shard runs none, and keeps no
        // log store of its own.
        cmd.env("CHLOEDB_START_APPS", "off").env("CHLOEDB_LOG_STORE", "off").env("CHLOEDB_GROWN", "1");
        if let Some(t) = &o.token {
            cmd.env("CHLOEDB_TOKEN", t);
        }
        let child = cmd.spawn().map_err(|e| format!("starting {} ({name}): {e}", o.bin.display()))?;
        self.children.push((name.to_string(), child));
        Ok(())
    }

    async fn spawn_coordinator(&mut self, o: &Options, args: &[String]) -> Result<(), String> {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(o.dir.join("coordinator.log")).map_err(|e| format!("coordinator.log: {e}"))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&o.bin);
        cmd.args(args).stdout(Stdio::from(log)).stderr(Stdio::from(err)).stdin(Stdio::null()).kill_on_drop(true);
        if let Some(t) = &o.token {
            cmd.env("CHLOEDB_TOKEN", t);
        }
        let child = cmd.spawn().map_err(|e| format!("starting {} (coordinator): {e}", o.bin.display()))?;
        self.children.push(("coordinator".into(), child));
        Ok(())
    }

    /// Waits a minute at most for a server to answer; a server that exits
    /// or does not answer is an error with the end of its log.
    async fn wait_up(&mut self, me: &Api, http: &str, dir: &Path, name: &str) -> Result<(), String> {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(60) {
            if me.get(&format!("{http}/v1/health")).await.is_ok() {
                return Ok(());
            }
            if let Some((_, c)) = self.children.iter_mut().find(|(n, _)| n == name) {
                if let Ok(Some(status)) = c.try_wait() {
                    return Err(format!("{name} stopped ({status}): {}", tail(&dir.join(format!("{name}.log")))));
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err(format!("{name} did not answer at {http} within a minute: {}", tail(&dir.join(format!("{name}.log")))))
    }

    /// Stops every server it started, the coordinator first.
    pub async fn stop(mut self) {
        let mut kids = std::mem::take(&mut self.children);
        kids.sort_by_key(|(n, _)| n != "coordinator");
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
            if tokio::time::timeout(Duration::from_secs(10), c.wait()).await.is_err() {
                tracing::warn!("{n} did not stop in 10 s: killed");
                let _ = c.kill().await;
            }
        }
    }

    /// The tables the cluster serves and their rows, for the banner.
    pub async fn served(&self) -> (Vec<String>, u64) {
        let Ok(http) = crate::http_client().no_proxy().timeout(Duration::from_secs(10)).build() else { return (vec![], 0) };
        let me = Api { http, token: self.token.clone() };
        let Ok(j) = me.get(&format!("{}/v1/health", self.url)).await else { return (vec![], 0) };
        let mut names = Vec::new();
        let mut rows = 0;
        for (name, n) in j.get("tables").and_then(Value::as_object).into_iter().flatten() {
            if chloedb::query::hint::internal_table(name) {
                continue;
            }
            names.push(name.to_string());
            rows += n.as_u64().unwrap_or(0);
        }
        (names, rows)
    }

    /// Whether a server it started has exited, and which.
    pub fn exited(&mut self) -> Option<String> {
        self.children.iter_mut().find_map(|(n, c)| matches!(c.try_wait(), Ok(Some(_))).then(|| n.clone()))
    }
}

fn shard_args(s: &Shard) -> Vec<String> {
    let addr = |u: &str| u.trim_start_matches("http://").to_string();
    vec!["--db".into(), s.db.clone(), "--seq-no".into(), "--listen".into(), addr(&s.http), "--grpc".into(), addr(&s.grpc)]
}

/// Renames the files of `db` (a shard's database that never joined, in
/// the cluster's folder) to end in `.unused-<seconds since 1970>`.
fn set_aside(dir: &Path, db: &Path) -> Result<(), String> {
    let Some(stem) = db.file_name().and_then(|n| n.to_str()) else { return Ok(()) };
    if db.parent() != Some(dir) {
        return Ok(());
    }
    let mut moved = false;
    let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == stem || (name.starts_with(stem) && matches!(name.as_bytes().get(stem.len()), Some(b'-' | b'.')) && !name.contains(".unused-")) {
            let to = dir.join(format!("{name}.unused-{when}"));
            std::fs::rename(e.path(), &to).map_err(|err| format!("setting {} aside: {err}", e.path().display()))?;
            moved = true;
        }
    }
    if moved {
        tracing::warn!("grow: {stem} was left by a grow that stopped part way and never joined the cluster; its files are set aside with the ending .unused-{when}, and a new one is made");
    }
    Ok(())
}

/// The last lines of a log, for an error.
fn tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    match lines.len() {
        0 => format!("nothing in {}", log.display()),
        n => format!("{} (in {})", lines[n.saturating_sub(3)..].join(" / "), log.display()),
    }
}

/// The user tables of a server, each with the statement that makes it.
async fn tables_of(me: &Api, http: &str) -> Result<Vec<(String, String)>, String> {
    let j = me.get(&format!("{http}/v1/tables")).await?;
    let list = j.get("tables").and_then(Value::as_array).cloned().or_else(|| j.as_array().cloned()).unwrap_or_default();
    let mut out = Vec::new();
    for t in list {
        let Some(name) = t.get("table").and_then(Value::as_str) else { continue };
        if chloedb::query::hint::internal_table(name) {
            continue;
        }
        let s = me.get(&format!("{http}/v1/schema?table={}", urlencode(name))).await?;
        let sql = s.get("sql").and_then(Value::as_str).ok_or_else(|| format!("{http} gave no statement for table {name}"))?;
        out.push((name.to_string(), sql.replacen("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ", 1)));
    }
    Ok(out)
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

#[derive(Clone)]
struct Api {
    http: reqwest::Client,
    token: Option<String>,
}

impl Api {
    async fn call(&self, r: reqwest::RequestBuilder) -> Result<Value, String> {
        let r = match &self.token {
            Some(t) => r.bearer_auth(t),
            None => r,
        };
        let resp = r.send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        let j: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(j.get("error").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("HTTP {status}")));
        }
        Ok(j)
    }

    async fn get(&self, url: &str) -> Result<Value, String> {
        self.call(self.http.get(url)).await
    }

    async fn post(&self, url: &str, body: &Value) -> Result<Value, String> {
        self.call(self.http.post(url).json(body)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shards_sit_above_the_clusters_port() {
        let dir = Path::new("/d/x.cluster");
        let s0 = shard_at("127.0.0.1:7700", dir, 0, Path::new("/d/x.chloedb")).unwrap();
        assert_eq!(s0, Shard { db: "/d/x.chloedb".into(), http: "http://127.0.0.1:7710".into(), grpc: "http://127.0.0.1:7711".into() });
        let s2 = shard_at("0.0.0.0:7700", dir, 2, Path::new("/d/x.chloedb")).unwrap();
        assert_eq!((s2.db.as_str(), s2.http.as_str()), ("/d/x.cluster/shard-2.chloedb", "http://127.0.0.1:7730"));
        assert!(shard_at("127.0.0.1:65530", dir, 0, Path::new("x")).is_err());
        assert_eq!(urlencode("my table"), "my%20table");
    }
}
