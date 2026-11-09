//! The cluster under test: chloedb-server processes on this machine, one
//! per copy of each shard, spare servers to replace copies with or to add
//! as shards, and coordinators, each run as a uid of its own (so that
//! firewall rules can single it out) with a data directory of its own
//! (on a tmpfs of `--disk-mb` when asked, so that its disk can fill).
//!
//! A supervisor starts again a process that exits, after a delay that
//! doubles while it keeps exiting soon after it starts, as a Kubernetes
//! StatefulSet would; a process held down (a lost node) stays down.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::observe::Events;

/// How the cluster is laid out and started.
#[derive(Clone, Debug)]
pub struct Layout {
    pub dir: PathBuf,
    pub bin: PathBuf,
    pub port: u16,
    pub shards: usize,
    pub copies: usize,
    pub spares: usize,
    pub coordinators: usize,
    pub rows: usize,
    pub failover_ms: u64,
    pub write_acks: String,
    pub disk_mb: u64,
    pub restart_ms: u64,
    pub admin_token: String,
    pub cluster_token: String,
    pub partial: bool,
    pub firewall: bool,
    /// Extra arguments for every server, and for every coordinator.
    pub server_args: Vec<String>,
    pub coordinator_args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A copy the cluster starts with: its shard and place.
    Copy { shard: usize, copy: usize },
    Spare,
    Coordinator,
}

/// One process of the cluster.
pub struct Proc {
    pub name: String,
    pub kind: Kind,
    pub http: u16,
    /// The real gRPC port, and the proxy's in front of it, which the
    /// cluster map names (0 for a coordinator).
    pub grpc: u16,
    pub proxy: u16,
    pub uid: u32,
    pub dir: PathBuf,
    child: Option<Child>,
    pub pid: Option<u32>,
    pub started: Option<Instant>,
    pub starts: u32,
    /// Held down by a fault or by hand: not started again.
    pub held: bool,
    restart_at: Option<Instant>,
    backoff_ms: u64,
    /// Starts with a database of its own already (not the first start).
    pub initialized: bool,
    /// Started again from nothing as a spare is (its volume was lost).
    pub as_spare: bool,
    /// Arguments a fault adds (bad configuration).
    pub extra: Vec<String>,
    /// A cluster token of its own instead of the cluster's (bad configuration).
    pub cluster_token: Option<String>,
    pub last_exit: Option<String>,
    pub mounted: bool,
}

impl Proc {
    pub fn db(&self) -> PathBuf {
        self.dir.join("db.strata")
    }

    pub fn grpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.proxy)
    }

    pub fn http_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    pub fn json(&self) -> Json {
        json!({
            "name": self.name, "kind": match &self.kind { Kind::Copy { shard, copy } => format!("copy {shard}/{copy}"), Kind::Spare => "spare".into(), Kind::Coordinator => "coordinator".into() },
            "http": self.http_url(), "grpc": if self.proxy > 0 { Json::from(self.grpc_url()) } else { Json::Null },
            "uid": self.uid, "pid": self.pid, "running": self.running(), "held": self.held, "starts": self.starts,
            "up_s": self.started.map(|s| s.elapsed().as_secs()), "last_exit": self.last_exit,
            "restart_in_ms": self.restart_at.map(|t| t.saturating_duration_since(Instant::now()).as_millis() as u64),
        })
    }
}

pub struct Cluster {
    pub layout: Layout,
    pub procs: Mutex<Vec<Proc>>,
    pub events: Arc<Events>,
    /// The shards' servers as the coordinators route by now (gRPC URLs),
    /// kept by the observer, for a coordinator started again.
    pub map: Mutex<Vec<Vec<String>>>,
    schema: PathBuf,
}

impl Cluster {
    pub fn new(layout: Layout, events: Arc<Events>) -> Result<Cluster, String> {
        std::fs::create_dir_all(layout.dir.join("logs")).map_err(|e| e.to_string())?;
        let schema = layout.dir.join("schema.json");
        std::fs::write(&schema, serde_json::to_string(&chloedb_server::demo::schema(layout.rows)).unwrap()).map_err(|e| e.to_string())?;
        let mut procs = Vec::new();
        let mut n = 0u16;
        let mut add = |name: String, kind: Kind, procs: &mut Vec<Proc>| {
            let is_coord = kind == Kind::Coordinator;
            let (http, grpc, proxy) = if is_coord {
                (layout.port + procs.iter().filter(|p: &&Proc| p.kind == Kind::Coordinator).count() as u16, 0, 0)
            } else {
                let b = layout.port + 100 + 4 * n;
                n += 1;
                (b, b + 1, b + 2)
            };
            let uid = 30_000 + procs.len() as u32;
            procs.push(Proc {
                dir: layout.dir.join(&name),
                name,
                kind,
                http,
                grpc,
                proxy,
                uid,
                child: None,
                pid: None,
                started: None,
                starts: 0,
                held: false,
                restart_at: None,
                backoff_ms: layout.restart_ms,
                initialized: false,
                as_spare: false,
                extra: Vec::new(),
                cluster_token: None,
                last_exit: None,
                mounted: false,
            });
        };
        // Nodes are named as machines are, by number: the command center
        // names copies by their place in the map (s0-1), which changes.
        for s in 0..layout.shards {
            for c in 0..layout.copies {
                add(format!("node{}", s * layout.copies + c), Kind::Copy { shard: s, copy: c }, &mut procs);
            }
        }
        for i in 0..layout.spares {
            add(format!("node{}", layout.shards * layout.copies + i), Kind::Spare, &mut procs);
        }
        for i in 0..layout.coordinators {
            add(format!("coord{i}"), Kind::Coordinator, &mut procs);
        }
        let map = (0..layout.shards).map(|s| procs.iter().filter(|p| matches!(p.kind, Kind::Copy { shard, .. } if shard == s)).map(|p| p.grpc_url()).collect()).collect();
        Ok(Cluster { layout, procs: Mutex::new(procs), events, map: Mutex::new(map), schema })
    }

    pub fn names(&self) -> Vec<String> {
        self.procs.lock().unwrap().iter().map(|p| p.name.clone()).collect()
    }

    pub fn with<R>(&self, name: &str, f: impl FnOnce(&mut Proc) -> R) -> Result<R, String> {
        let mut ps = self.procs.lock().unwrap();
        let p = ps.iter_mut().find(|p| p.name == name).ok_or_else(|| format!("no process named {name}"))?;
        Ok(f(p))
    }

    /// The name of the process whose proxy (or HTTP port) a URL names.
    pub fn name_of_url(&self, url: &str) -> Option<String> {
        let port: u16 = url.rsplit(':').next()?.trim_end_matches('/').parse().ok()?;
        self.procs.lock().unwrap().iter().find(|p| p.proxy == port || p.http == port || p.grpc == port).map(|p| p.name.clone())
    }

    pub fn coordinators(&self) -> Vec<String> {
        self.procs.lock().unwrap().iter().filter(|p| p.kind == Kind::Coordinator).map(|p| p.http_url()).collect()
    }

    /// Starts every process, the shards' copies first, and waits until
    /// every server answers `/readyz`.
    pub fn start_all(&self) -> Result<(), String> {
        let names: Vec<(String, bool)> = self.procs.lock().unwrap().iter().map(|p| (p.name.clone(), p.kind == Kind::Coordinator)).collect();
        for (n, coord) in &names {
            if !coord {
                self.start(n)?;
            }
        }
        for (n, coord) in &names {
            if !coord {
                self.wait_ready(n, Duration::from_secs(120))?;
            }
        }
        for (n, coord) in &names {
            if *coord {
                self.start(n)?;
            }
        }
        for (n, coord) in &names {
            if *coord {
                self.wait_ready(n, Duration::from_secs(60))?;
            }
        }
        Ok(())
    }

    pub fn wait_ready(&self, name: &str, within: Duration) -> Result<(), String> {
        let url = self.with(name, |p| format!("{}/readyz", p.http_url()))?;
        let t = Instant::now();
        while t.elapsed() < within {
            if ureq_ok(&url) {
                return Ok(());
            }
            let exited = self.with(name, |p| p.child.as_mut().and_then(|c| c.try_wait().ok().flatten()))?;
            if let Some(st) = exited {
                return Err(format!("{name} exited ({}) as it started; its log ends:\n{}", exit_text(st), self.log_tail(name, 20)));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let log = self.log_tail(name, 20);
        Err(format!("{name} did not become ready in {} s; its log ends:\n{log}", within.as_secs()))
    }

    pub fn log_path(&self, name: &str) -> PathBuf {
        self.layout.dir.join("logs").join(format!("{name}.log"))
    }

    pub fn log_tail(&self, name: &str, n: usize) -> String {
        let text = std::fs::read_to_string(self.log_path(name)).unwrap_or_default();
        let lines: Vec<&str> = text.lines().rev().take(n).collect();
        lines.into_iter().rev().collect::<Vec<_>>().join("\n")
    }

    /// Starts a process that does not run.
    pub fn start(&self, name: &str) -> Result<(), String> {
        let l = &self.layout;
        let map = self.map.lock().unwrap().clone();
        let mut ps = self.procs.lock().unwrap();
        let shard_urls: BTreeMap<usize, Vec<String>> = (0..l.shards).map(|s| (s, ps.iter().filter(|p| matches!(p.kind, Kind::Copy { shard, .. } if shard == s)).map(|p| p.grpc_url()).collect())).collect();
        let p = ps.iter_mut().find(|p| p.name == name).ok_or_else(|| format!("no process named {name}"))?;
        if p.child.is_some() {
            return Ok(());
        }
        std::fs::create_dir_all(&p.dir).map_err(|e| e.to_string())?;
        if l.disk_mb > 0 && !p.mounted && p.kind != Kind::Coordinator {
            mount_tmpfs(&p.dir, l.disk_mb)?;
            p.mounted = true;
        }
        chown_r(&p.dir, p.uid);
        let mut cmd = Command::new(&l.bin);
        cmd.env("HOME", &p.dir).env_remove("STRATA_TOKEN").env_remove("STRATA_CLUSTER_TOKEN");
        cmd.args(["--listen", &format!("127.0.0.1:{}", p.http), "--log", "info", "--node-name", &p.name, "--cluster-name", "chaos", "--environment", "chaos"]);
        cmd.args(["--cluster-token", p.cluster_token.as_deref().unwrap_or(&l.cluster_token)]);
        match &p.kind {
            Kind::Coordinator => {
                let list: Vec<String> = map.iter().map(|s| s.join("|")).collect();
                cmd.args(["--shards", &list.join(","), "--grpc", "off", "--token", &l.admin_token, "--peer-url", &p.http_url()]);
                cmd.arg("--audit-log").arg(p.dir.join("audit.jsonl"));
                cmd.arg("--alerts").arg(p.dir.join("alerts.json"));
                cmd.arg("--contain").arg(p.dir.join("contain.json"));
                cmd.arg("--log-store").arg(p.dir.join("coordinator.logs"));
                if l.partial {
                    cmd.arg("--partial");
                }
                cmd.args(&l.coordinator_args);
            }
            kind => {
                cmd.arg("--db").arg(p.db()).args(["--seq-no", "--grpc", &format!("127.0.0.1:{}", p.grpc), "--failover-after-ms", &l.failover_ms.to_string(), "--write-acks", &l.write_acks]);
                cmd.arg("--backup-dir").arg(p.dir.join("backups"));
                let fresh = !p.db().exists();
                match kind {
                    Kind::Copy { shard, copy } if fresh && !p.as_spare => {
                        cmd.args(["--demo", &l.rows.to_string(), "--demo-shard", &format!("{shard}/{}", l.shards)]);
                        if *copy == 0 {
                            let others: Vec<String> = shard_urls[shard].iter().skip(1).cloned().collect();
                            if !others.is_empty() {
                                cmd.args(["--replicas", &others.join(",")]);
                            }
                        } else {
                            cmd.arg("--replica");
                        }
                    }
                    _ if fresh => {
                        cmd.arg("--schema").arg(&self.schema);
                    }
                    _ => {}
                }
                cmd.args(&l.server_args);
            }
        }
        cmd.args(&p.extra);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.layout.dir.join("logs").join(format!("{}.log", p.name))).map_err(|e| e.to_string())?;
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
        let uid = p.uid;
        #[cfg(unix)]
        unsafe {
            use std::os::unix::process::CommandExt;
            let root = libc::geteuid() == 0;
            cmd.pre_exec(move || {
                // Its own process group, so that a Ctrl-C to the monkey
                // does not reach it before the monkey stops it.
                libc::setpgid(0, 0);
                if root {
                    if libc::setgid(uid) != 0 || libc::setuid(uid) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("start {}: {e}", p.name))?;
        p.pid = Some(child.id());
        p.child = Some(child);
        p.started = Some(Instant::now());
        p.starts += 1;
        p.initialized = true;
        p.restart_at = None;
        let (n, pid, starts) = (p.name.clone(), p.pid, p.starts);
        drop(ps);
        self.events.add("process", &format!("{n} started{}", if starts > 1 { format!(" (start {starts})") } else { String::new() }), json!({"name": n, "pid": pid}));
        Ok(())
    }

    /// Sends a signal to a process; `SIGKILL` and `SIGTERM` leave it to
    /// the supervisor, which starts it again unless it is held.
    pub fn signal(&self, name: &str, sig: i32) -> Result<(), String> {
        let pid = self.with(name, |p| p.pid)?.ok_or_else(|| format!("{name} does not run"))?;
        let r = unsafe { libc::kill(pid as i32, sig) };
        if r != 0 {
            return Err(format!("signal {sig} to {name}: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Holds a process down (killing it first), or lets the supervisor
    /// start it again.
    pub fn hold(&self, name: &str, held: bool) -> Result<(), String> {
        self.with(name, |p| {
            p.held = held;
            if !held {
                p.restart_at = Some(Instant::now());
                p.backoff_ms = self.layout.restart_ms;
            }
        })?;
        if held {
            let _ = self.signal(name, libc::SIGKILL);
            self.reap_one(name, Duration::from_secs(5));
        }
        Ok(())
    }

    fn reap_one(&self, name: &str, within: Duration) {
        let t = Instant::now();
        while t.elapsed() < within {
            let done = self.with(name, |p| match p.child.as_mut().map(|c| c.try_wait()) {
                Some(Ok(Some(_))) | None => true,
                _ => false,
            });
            if done.unwrap_or(true) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.supervise_once();
    }

    /// Reaps processes that exited and starts again those due.
    pub fn supervise_once(&self) {
        let now = Instant::now();
        let mut exited = Vec::new();
        let mut due = Vec::new();
        {
            let mut ps = self.procs.lock().unwrap();
            for p in ps.iter_mut() {
                if let Some(c) = p.child.as_mut() {
                    if let Ok(Some(st)) = c.try_wait() {
                        let why = exit_text(st);
                        let lived = p.started.map(|s| s.elapsed()).unwrap_or_default();
                        // Exits soon after it started: back off as a crash loop.
                        p.backoff_ms = if lived < Duration::from_secs(10) && p.starts > 1 { (p.backoff_ms * 2).min(60_000) } else { self.layout.restart_ms };
                        p.child = None;
                        p.pid = None;
                        p.last_exit = Some(why.clone());
                        if !p.held {
                            p.restart_at = Some(now + Duration::from_millis(p.backoff_ms));
                        }
                        exited.push((p.name.clone(), why, lived, if p.held { None } else { Some(p.backoff_ms) }));
                    }
                } else if !p.held && p.restart_at.is_some_and(|t| t <= now) {
                    due.push(p.name.clone());
                }
            }
        }
        for (n, why, lived, again) in exited {
            let text = match again {
                Some(ms) => format!("{n} exited ({why}) after {:.1} s; starting again in {:.1} s", lived.as_secs_f64(), ms as f64 / 1000.0),
                None => format!("{n} exited ({why}) after {:.1} s; held down", lived.as_secs_f64()),
            };
            self.events.add("process", &text, json!({"name": n, "exit": why}));
        }
        for n in due {
            if let Err(e) = self.start(&n) {
                self.events.add("process", &format!("{n} did not start: {e}"), json!({"name": n}));
                let _ = self.with(&n, |p| p.restart_at = Some(Instant::now() + Duration::from_millis(p.backoff_ms)));
            }
        }
    }

    /// Stops every process and unmounts their disks.
    pub fn shutdown(&self) {
        let mut ps = self.procs.lock().unwrap();
        for p in ps.iter_mut() {
            p.held = true;
            if let Some(c) = p.child.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
            p.child = None;
            p.pid = None;
        }
        for p in ps.iter_mut() {
            if p.mounted {
                let _ = Command::new("umount").arg("-l").arg(&p.dir).status();
                p.mounted = false;
            }
        }
    }

    /// The data directory's files, largest first.
    pub fn segment_files(&self, name: &str) -> Vec<PathBuf> {
        let dir = self.with(name, |p| p.dir.clone()).unwrap_or_default();
        let mut files = Vec::new();
        walk(&dir, &mut files);
        files.retain(|(p, _)| {
            let s = p.to_string_lossy();
            s.contains("db.strata") && !s.ends_with(".replication") && !s.ends_with(".slots") && !s.ends_with(".json") && !s.contains(".logs")
        });
        files.sort_by(|a, b| b.1.cmp(&a.1));
        files.into_iter().map(|(p, _)| p).collect()
    }
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if let Ok(m) = e.metadata() {
            out.push((p, m.len()));
        }
    }
}

fn exit_text(st: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(s) = st.signal() {
            return match s {
                libc::SIGKILL => "killed".into(),
                libc::SIGTERM => "terminated".into(),
                libc::SIGABRT => "aborted".into(),
                libc::SIGSEGV => "segfault".into(),
                s => format!("signal {s}"),
            };
        }
    }
    match st.code() {
        Some(0) => "exit 0".into(),
        Some(c) => format!("exit {c}"),
        None => "exited".into(),
    }
}

fn mount_tmpfs(dir: &Path, mb: u64) -> Result<(), String> {
    let st = Command::new("mount").args(["-t", "tmpfs", "-o", &format!("size={mb}m"), "tmpfs"]).arg(dir).status().map_err(|e| format!("mount: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("mount a tmpfs of {mb} MiB on {} failed (needs root on Linux; leave --disk-mb at 0 elsewhere)", dir.display()))
    }
}

pub fn chown_r(dir: &Path, uid: u32) {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let _ = Command::new("chown").arg("-R").arg(format!("{uid}:{uid}")).arg(dir).status();
}

/// A blocking GET that says whether it answered 2xx within 2 s.
pub fn ureq_ok(url: &str) -> bool {
    // A plain HTTP/1.1 GET over a socket: no runtime needed here.
    use std::io::{Read, Write};
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let Ok(addr) = host.parse::<std::net::SocketAddr>() else { return false };
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 16];
    let n = s.read(&mut buf).unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]);
    head.starts_with("HTTP/1.1 2") || head.starts_with("HTTP/1.0 2")
}
