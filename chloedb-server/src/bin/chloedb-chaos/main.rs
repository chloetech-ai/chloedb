//! chloedb-chaos: a chaos monkey for a ChloeDB cluster.
//!
//! It starts a cluster of chloedb-server processes on this machine (shards
//! of three copies with automatic failover, spare servers, two
//! coordinators), puts clients on it that search, read by key and write
//! counters it checks, and then breaks things: processes crash, stall or
//! slow down, links are cut or delayed, disks fill, files rot, volumes
//! are lost, configuration goes wrong, clients flood the cluster. It
//! records what the clients saw, second by second, and what the cluster
//! said of itself (the command center's Home, its alerts and its map), so
//! that each fault can be read as an operator meets it.
//!
//! ```text
//! chloedb-chaos up --dir /tmp/chaos          # a cluster, clients, and the monkey's control API; Ctrl-C ends it
//! chloedb-chaos inject kill primary:0        # from another shell: a fault, as the cluster is now
//! chloedb-chaos inject delay replica:1 --ms 300
//! chloedb-chaos inject isolate primary:1
//! chloedb-chaos status                       # processes, the map, faults active, what the clients see
//! chloedb-chaos heal --all
//! chloedb-chaos check                        # the counters against what was acknowledged
//! chloedb-chaos run --dir /tmp/chaos         # every scenario in turn, then results.json and results.md
//! chloedb-chaos run --monkey 1800 --seed 7   # 30 minutes of random faults, then the check
//! chloedb-chaos faults                       # what it can break
//! ```
//!
//! Cutting links (iptables) and filling disks (a tmpfs per server,
//! `--disk-mb`) need Linux and root; everything else runs anywhere.

mod cluster;
mod faults;
mod net;
mod observe;
mod scenarios;
mod workload;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value as Json};

use cluster::{Cluster, Layout};
use faults::{Inject, Monkey};
use net::{Firewall, Proxies};
use observe::{Events, Seen};
use workload::Workload;

#[derive(Parser)]
#[command(name = "chloedb-chaos", about = "A chaos monkey for a ChloeDB cluster of local processes")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start a cluster, its clients and the control API, until Ctrl-C.
    Up(Setup),
    /// Run the scenarios (or random faults) on a cluster of its own, then
    /// write results.json and results.md into --dir.
    Run(RunArgs),
    /// Inject a fault into the cluster `up` runs.
    Inject {
        fault: String,
        target: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        oneway: bool,
        #[arg(long)]
        ms: Option<u64>,
        #[arg(long)]
        pct: Option<u64>,
        #[arg(long)]
        free_mb: Option<u64>,
        #[arg(long)]
        clients: Option<usize>,
        #[arg(long)]
        shard: Option<usize>,
        #[command(flatten)]
        c: Control,
    },
    /// Undo a fault, or every fault.
    Heal {
        id: Option<String>,
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        c: Control,
    },
    /// Processes, the map, active faults, and the clients' last 10 s.
    Status {
        #[command(flatten)]
        c: Control,
    },
    /// Compare the counters with what was acknowledged (writes wait meanwhile).
    Check {
        #[command(flatten)]
        c: Control,
    },
    /// Write a note into the run's events (what an operator did, say).
    Mark {
        text: Vec<String>,
        #[command(flatten)]
        c: Control,
    },
    /// Start, stop (hold down), release or kill one process by name, as
    /// an operator with a shell (or kubectl) would.
    Proc {
        name: String,
        /// start, hold, release, kill or term
        action: String,
        #[command(flatten)]
        c: Control,
    },
    /// The events since --since seconds into the run.
    Events {
        #[arg(long, default_value_t = 0.0)]
        since: f64,
        #[command(flatten)]
        c: Control,
    },
    /// What the clients saw over the last --last seconds.
    Stats {
        #[arg(long, default_value_t = 60)]
        last: u64,
        #[command(flatten)]
        c: Control,
    },
    /// The faults it can inject.
    Faults,
}

#[derive(Args, Clone)]
struct Control {
    /// The control API of `up` (its --port + 99).
    #[arg(long, env = "CHLOEDB_CHAOS_CONTROL", default_value = "http://127.0.0.1:7999")]
    control: String,
}

#[derive(Args, Clone)]
struct Setup {
    /// Where the servers' data, logs and the run's records go (emptied first).
    #[arg(long, default_value = "/tmp/chloedb-chaos")]
    dir: PathBuf,
    /// The chloedb-server binary (default: the one beside this one).
    #[arg(long)]
    bin: Option<PathBuf>,
    /// The first port: coordinators from it, the control API at +99, servers from +100.
    #[arg(long, default_value_t = 7900)]
    port: u16,
    #[arg(long, default_value_t = 2)]
    shards: usize,
    /// Copies of each shard.
    #[arg(long, default_value_t = 3)]
    copies: usize,
    /// Servers started empty, to replace copies with or to add as shards.
    #[arg(long, default_value_t = 2)]
    spares: usize,
    #[arg(long, default_value_t = 2)]
    coordinators: usize,
    /// Demo products, split over the shards.
    #[arg(long, default_value_t = 20_000)]
    rows: usize,
    #[arg(long, default_value_t = 2000)]
    failover_ms: u64,
    #[arg(long, default_value = "all")]
    write_acks: String,
    /// A tmpfs of this many MiB for each server's data (Linux, root): its
    /// disk use is then its own, and disk-fill can fill it. 0: none.
    #[arg(long, default_value_t = 0)]
    disk_mb: u64,
    /// How long a process that exited waits before it starts again.
    #[arg(long, default_value_t = 5000)]
    restart_after_ms: u64,
    /// Coordinators answer without a shard that does not answer (--partial).
    #[arg(long)]
    partial: bool,
    /// No firewall rules (cut and isolate fail).
    #[arg(long)]
    no_firewall: bool,
    #[arg(long, default_value_t = 4)]
    readers: usize,
    #[arg(long, default_value_t = 2)]
    writers: usize,
    /// Calls a second of each reader and each writer.
    #[arg(long, default_value_t = 20.0)]
    rate: f64,
    #[arg(long, default_value_t = 400)]
    counters: usize,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// The coordinators' admin token (default: drawn).
    #[arg(long, env = "CHLOEDB_CHAOS_TOKEN")]
    token: Option<String>,
    /// Extra arguments for every server.
    #[arg(long, allow_hyphen_values = true, value_delimiter = ' ')]
    server_args: Vec<String>,
}

#[derive(Args, Clone)]
struct RunArgs {
    #[command(flatten)]
    setup: Setup,
    /// The scenarios to run, by name (default: every one); `--list` lists them.
    #[arg(long, value_delimiter = ',')]
    scenarios: Vec<String>,
    #[arg(long)]
    list: bool,
    /// Instead of the scenarios: random faults for this many seconds.
    #[arg(long)]
    monkey: Option<u64>,
    /// Mean seconds between two random faults.
    #[arg(long, default_value_t = 20)]
    gap_s: u64,
}

/// Everything a running cluster is made of.
pub struct World {
    pub cluster: Arc<Cluster>,
    pub monkey: Arc<Monkey>,
    pub work: Arc<Workload>,
    pub seen: Arc<Seen>,
    pub events: Arc<Events>,
    pub firewall: Arc<Firewall>,
    pub http: reqwest::Client,
    pub stop: Arc<AtomicBool>,
}

impl World {
    /// An admin call to a coordinator that answers, as the command center's
    /// buttons make it (with the admin token).
    pub async fn admin(&self, method: reqwest::Method, path: &str, body: Option<Json>) -> Result<Json, String> {
        let mut last = String::from("no coordinator");
        for c in self.cluster.coordinators() {
            let mut req = self.http.request(method.clone(), format!("{c}{path}")).header("authorization", format!("Bearer {}", self.cluster.layout.admin_token)).header("x-strata-client", "chloedb-chaos").timeout(Duration::from_secs(60));
            if let Some(b) = &body {
                req = req.json(b);
            }
            match req.send().await {
                Ok(r) => {
                    let st = r.status();
                    let j: Json = r.json().await.unwrap_or(Json::Null);
                    return if st.is_success() { Ok(j) } else { Err(format!("{st}: {}", j["error"].as_str().map(str::to_string).unwrap_or_else(|| j.to_string()))) };
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(last)
    }

    pub fn status(&self) -> Json {
        let procs: Vec<Json> = self.cluster.procs.lock().unwrap().iter().map(|p| p.json()).collect();
        let look = self.seen.last.lock().unwrap().clone();
        let now = self.events.start.elapsed().as_secs();
        json!({
            "t": (self.events.t() * 10.0).round() / 10.0,
            "ui": format!("{}/?token={}", self.cluster.coordinators()[0], self.cluster.layout.admin_token),
            "processes": procs,
            "map": {"epoch": look.epoch, "shards": look.shards},
            "home": {"status": look.status, "headline": look.headline, "problems": look.problems, "alerts": look.alerts, "asked": look.coordinator},
            "faults": self.monkey.active(),
            "clients_last_10s": self.work.stats.summary(now.saturating_sub(10), now + 1),
            "client_errors": self.work.stats.samples.lock().unwrap().iter().map(|(k, (m, t))| (k.clone(), json!({"text": m, "at_s": (t * 10.0).round() / 10.0}))).collect::<BTreeMap<_, _>>(),
        })
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.work.stop.store(true, Ordering::SeqCst);
        self.monkey.heal_all();
        self.cluster.shutdown();
        self.firewall.teardown();
    }
}

fn token() -> String {
    use rand::Rng;
    let mut r = rand::thread_rng();
    (0..24).map(|_| format!("{:x}", r.gen_range(0..16))).collect()
}

fn unmount_under(dir: &std::path::Path) {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return };
    let prefix = dir.to_string_lossy().to_string();
    for line in mounts.lines() {
        if let Some(m) = line.split_whitespace().nth(1) {
            if m.starts_with(&prefix) {
                let _ = std::process::Command::new("umount").arg("-l").arg(m).status();
            }
        }
    }
}

/// Starts the cluster, the proxies, the supervisor, the observer and the
/// clients.
async fn setup(s: &Setup, quiet: bool) -> Result<Arc<World>, String> {
    let bin = match &s.bin {
        Some(b) => b.clone(),
        None => std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("chloedb-server"),
    };
    if !bin.exists() {
        return Err(format!("no chloedb-server at {} (build it: cargo build --profile ci --bin chloedb-server, or give --bin)", bin.display()));
    }
    // Each server runs as a uid of its own: it must reach its directory.
    if unsafe { libc::geteuid() } == 0 {
        let abs = std::path::absolute(&s.dir).map_err(|e| e.to_string())?;
        for a in abs.ancestors().skip(1) {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = std::fs::metadata(a) {
                if m.permissions().mode() & 0o001 == 0 {
                    return Err(format!("{} is not open to other users (mode {:o}), so the servers, each run as a uid of its own, cannot reach --dir; pick a --dir under /tmp", a.display(), m.permissions().mode() & 0o777));
                }
            }
        }
    }
    unmount_under(&s.dir);
    let _ = std::fs::remove_dir_all(&s.dir);
    std::fs::create_dir_all(&s.dir).map_err(|e| e.to_string())?;
    let events = Arc::new(Events::new(&s.dir.join("events.jsonl"), quiet));
    let layout = Layout {
        dir: s.dir.clone(),
        bin,
        port: s.port,
        shards: s.shards,
        copies: s.copies,
        spares: s.spares,
        coordinators: s.coordinators.max(1),
        rows: s.rows,
        failover_ms: s.failover_ms,
        write_acks: s.write_acks.clone(),
        disk_mb: s.disk_mb,
        restart_ms: s.restart_after_ms,
        admin_token: s.token.clone().unwrap_or_else(token),
        cluster_token: token(),
        partial: s.partial,
        firewall: !s.no_firewall,
        server_args: s.server_args.iter().filter(|a| !a.is_empty()).cloned().collect(),
        coordinator_args: Vec::new(),
    };
    let firewall = Arc::new(Firewall::new(layout.firewall));
    if layout.firewall && !firewall.enabled {
        events.add("note", "no firewall (needs iptables, Linux and root): cut and isolate will fail", json!({}));
    }
    let cluster = Arc::new(Cluster::new(layout, events.clone())?);
    let proxies = Arc::new(Proxies::default());
    let pairs: Vec<(u16, u16)> = cluster.procs.lock().unwrap().iter().filter(|p| p.proxy > 0).map(|p| (p.proxy, p.grpc)).collect();
    for (listen, target) in pairs {
        proxies.start(listen, target).await.map_err(|e| format!("proxy on {listen}: {e}"))?;
    }
    events.add("note", &format!("starting {} shards of {} copies, {} spares, {} coordinators, {} rows", s.shards, s.copies, s.spares, s.coordinators, s.rows), json!({}));
    let c2 = cluster.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || c2.start_all()).await.map_err(|e| e.to_string()).and_then(|r| r) {
        cluster.shutdown();
        firewall.teardown();
        return Err(e);
    }
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (c, stop) = (cluster.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                c.supervise_once();
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    }
    let http = chloedb_server::http_client().timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
    let seen = Arc::new(Seen::default());
    tokio::spawn(observe::observe(cluster.clone(), seen.clone(), http.clone(), stop.clone()));
    // The clients' keys, made as an operator makes them.
    let mut keys = BTreeMap::new();
    for (name, scope) in [("app-search", "read"), ("app-writer", "write"), ("nightly-report", "read")] {
        let coord = &cluster.coordinators()[0];
        let r = http.post(format!("{coord}/v1/admin/keys")).header("authorization", format!("Bearer {}", cluster.layout.admin_token)).json(&json!({"name": name, "scope": scope})).send().await.map_err(|e| e.to_string())?;
        let j: Json = r.json().await.map_err(|e| e.to_string())?;
        let secret = j["secret"].as_str().map(str::to_string).ok_or_else(|| format!("making key {name}: {j}"))?;
        keys.insert(name.to_string(), secret);
    }
    // Every coordinator holds the keys once they merge (every 5 s).
    tokio::time::sleep(Duration::from_secs(6)).await;
    let cfg = workload::Config { readers: s.readers, writers: s.writers, rate: s.rate, counters: s.counters, rows: s.rows, seed: s.seed };
    let work = Workload::new(cluster.coordinators(), &cfg, events.clone(), keys);
    for i in 0..cfg.readers {
        tokio::spawn(work.clone().reader(cfg.seed * 1000 + i as u64));
    }
    for i in 0..cfg.writers {
        tokio::spawn(work.clone().writer(i, cfg.seed));
    }
    let monkey = Monkey::new(cluster.clone(), proxies, firewall.clone(), work.clone(), seen.clone());
    // Until the observer has seen the map, targets by role do not resolve.
    for _ in 0..50 {
        if !seen.last.lock().unwrap().shards.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    events.add("note", &format!("clients started: {} readers, {} writers, {} calls a second each", cfg.readers, cfg.writers, cfg.rate), json!({}));
    Ok(Arc::new(World { cluster, monkey, work, seen, events, firewall, http, stop }))
}

type W = Arc<World>;

fn control_api(w: W) -> Router {
    Router::new()
        .route("/status", get(|State(w): State<W>| async move { AxJson(w.status()) }))
        .route(
            "/inject",
            post(|State(w): State<W>, AxJson(r): AxJson<Inject>| async move {
                match w.monkey.inject(&r) {
                    Ok(j) => AxJson(j),
                    Err(e) => AxJson(json!({"error": e})),
                }
            }),
        )
        .route(
            "/heal",
            post(|State(w): State<W>, AxJson(b): AxJson<Json>| async move {
                if b["all"].as_bool() == Some(true) {
                    return AxJson(json!({"healed": w.monkey.heal_all()}));
                }
                match w.monkey.heal(b["id"].as_str().unwrap_or("")) {
                    Ok(j) => AxJson(j),
                    Err(e) => AxJson(json!({"error": e})),
                }
            }),
        )
        .route(
            "/check",
            post(|State(w): State<W>| async move {
                let r = w.work.check().await;
                w.events.add("check", &format!("check: {}", check_text(&r)), r.clone());
                AxJson(r)
            }),
        )
        .route(
            "/mark",
            post(|State(w): State<W>, AxJson(b): AxJson<Json>| async move {
                let text = b["text"].as_str().unwrap_or("").to_string();
                w.events.add("note", &text, json!({}));
                AxJson(json!({"t": w.events.t()}))
            }),
        )
        .route(
            "/proc",
            post(|State(w): State<W>, AxJson(b): AxJson<Json>| async move {
                let (name, action) = (b["name"].as_str().unwrap_or(""), b["action"].as_str().unwrap_or(""));
                let r = match action {
                    "start" | "release" => w.cluster.hold(name, false),
                    "hold" | "stop" => w.cluster.hold(name, true),
                    "kill" => w.cluster.signal(name, libc::SIGKILL),
                    "term" => w.cluster.signal(name, libc::SIGTERM),
                    a => Err(format!("no action {a}: start, hold, release, kill or term")),
                };
                if r.is_ok() {
                    w.events.add("operator", &format!("{name}: {action} (by hand)"), json!({"name": name, "action": action}));
                }
                AxJson(match r {
                    Ok(()) => json!({"ok": true}),
                    Err(e) => json!({"error": e}),
                })
            }),
        )
        .route("/events", get(|State(w): State<W>, Query(q): Query<BTreeMap<String, String>>| async move { AxJson(Json::Array(w.events.since(q.get("since").and_then(|s| s.parse().ok()).unwrap_or(0.0)))) }))
        .route(
            "/stats",
            get(|State(w): State<W>, Query(q): Query<BTreeMap<String, u64>>| async move {
                let now = w.events.start.elapsed().as_secs();
                let from = q.get("from").copied().unwrap_or(now.saturating_sub(q.get("last").copied().unwrap_or(60)));
                let to = q.get("to").copied().unwrap_or(now + 1);
                AxJson(json!({"from": from, "to": to, "summary": w.work.stats.summary(from, to), "series": w.work.stats.series(from, to)}))
            }),
        )
        .with_state(w)
}

pub fn check_text(r: &Json) -> String {
    if let Some(e) = r["error"].as_str() {
        return format!("could not read the counters: {e}");
    }
    format!(
        "{} counters, {} adds acknowledged, {} may have applied, sum {}: {} lost, {} applied twice",
        r["counters"], r["acknowledged"], r["may_have_applied"], r["sum"],
        r["lost"].as_array().map_or(0, |a| a.len()), r["applied_twice"].as_array().map_or(0, |a| a.len())
    )
}

async fn client(c: &Control, method: reqwest::Method, path: &str, body: Option<Json>) -> Json {
    let http = reqwest::Client::new();
    let mut req = http.request(method, format!("{}{path}", c.control.trim_end_matches('/'))).timeout(Duration::from_secs(120));
    if let Some(b) = body {
        req = req.json(&b);
    }
    match req.send().await {
        Ok(r) => r.json().await.unwrap_or(Json::Null),
        Err(e) => {
            eprintln!("chloedb-chaos: no control API at {} ({e}); is `chloedb-chaos up` running?", c.control);
            std::process::exit(2);
        }
    }
}

fn print(j: &Json) {
    println!("{}", serde_json::to_string_pretty(j).unwrap_or_default());
    if j.get("error").is_some() {
        std::process::exit(1);
    }
}

fn main() {
    // Settings may be named CHLOEDB_* (the name since the rename) or STRATA_*:
    // clap and the code read the old names, so the new ones are copied over
    // before the runtime starts any thread.
    chloedb::env::adopt_old_names();
    run();
}

#[tokio::main]
async fn run() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Faults => {
            for (name, args, what) in faults::CATALOG {
                println!("{name:<10} {args:<32} {what}");
            }
            println!("\nTargets: a name (node3, coord1), primary:N, replica:N, replicas:N, shard:N, coord, any-copy, any-replica.");
        }
        Cmd::Up(s) => {
            let w = match setup(&s, false).await {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("chloedb-chaos: {e}");
                    std::process::exit(1);
                }
            };
            let port = s.port + 99;
            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("chloedb-chaos: control API on {port}: {e}");
                    w.shutdown();
                    std::process::exit(1);
                }
            };
            let st = w.status();
            eprintln!("\ncommand center: {}\ncontrol API:    http://127.0.0.1:{port} (chloedb-chaos status)\n", st["ui"].as_str().unwrap_or(""));
            for p in st["processes"].as_array().into_iter().flatten() {
                eprintln!("  {:<7} {:<12} http {}  grpc {}", p["name"].as_str().unwrap_or(""), p["kind"].as_str().unwrap_or(""), p["http"].as_str().unwrap_or(""), p["grpc"].as_str().unwrap_or("-"));
            }
            let _ = std::fs::write(s.dir.join("status.json"), serde_json::to_string_pretty(&st).unwrap_or_default());
            let app = control_api(w.clone());
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
            tokio::select! {
                _ = axum::serve(listener, app) => {}
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            eprintln!("stopping the cluster");
            let r = w.work.check().await;
            w.events.add("check", &format!("check: {}", check_text(&r)), r);
            w.shutdown();
        }
        Cmd::Run(r) => {
            if r.list {
                for s in scenarios::all() {
                    println!("{:<20} {}", s.name, s.title);
                }
                return;
            }
            let w = match setup(&r.setup, false).await {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("chloedb-chaos: {e}");
                    std::process::exit(1);
                }
            };
            // The control API runs during a run too, to look in.
            if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", r.setup.port + 99)).await {
                let app = control_api(w.clone());
                tokio::spawn(async move { axum::serve(l, app).await });
            }
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
            let ok = tokio::select! {
                ok = async {
                    if let Some(secs) = r.monkey {
                        scenarios::monkey(&w, secs, r.setup.seed, r.gap_s, &r.setup.dir).await
                    } else {
                        scenarios::run(&w, &r.scenarios, &r.setup.dir).await
                    }
                } => ok,
                _ = tokio::signal::ctrl_c() => false,
                _ = term.recv() => false,
            };
            w.shutdown();
            if !ok {
                std::process::exit(1);
            }
        }
        Cmd::Inject { fault, target, to, oneway, ms, pct, free_mb, clients, shard, c } => {
            print(&client(&c, reqwest::Method::POST, "/inject", Some(serde_json::to_value(Inject { fault, target, to, oneway, ms, pct, free_mb, clients, shard }).unwrap())).await);
        }
        Cmd::Heal { id, all, c } => print(&client(&c, reqwest::Method::POST, "/heal", Some(json!({"id": id, "all": all}))).await),
        Cmd::Status { c } => print(&client(&c, reqwest::Method::GET, "/status", None).await),
        Cmd::Check { c } => print(&client(&c, reqwest::Method::POST, "/check", None).await),
        Cmd::Mark { text, c } => print(&client(&c, reqwest::Method::POST, "/mark", Some(json!({"text": text.join(" ")}))).await),
        Cmd::Proc { name, action, c } => print(&client(&c, reqwest::Method::POST, "/proc", Some(json!({"name": name, "action": action}))).await),
        Cmd::Events { since, c } => {
            let j = client(&c, reqwest::Method::GET, &format!("/events?since={since}"), None).await;
            for e in j.as_array().into_iter().flatten() {
                println!("[{:>7.1}s] {:<8} {}", e["t"].as_f64().unwrap_or(0.0), e["kind"].as_str().unwrap_or(""), e["text"].as_str().unwrap_or(""));
            }
        }
        Cmd::Stats { last, c } => print(&client(&c, reqwest::Method::GET, &format!("/stats?last={last}"), None).await["summary"]),
    }
}
