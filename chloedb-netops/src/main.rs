//! `chloedb-netops serve`: the network operations app's API and view, for
//! ChloeDB's command center.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use tokio::process::{Child, Command};

use chloedb_netops::api::{self, Service};
use chloedb_netops::sim::{Engine, Opts};
use chloedb_netops::strata::Strata;

#[derive(Parser)]
#[command(name = "chloedb-netops", version, about = "Network operations on ChloeDB: an app of its command center. Topology, telemetry rollups, alert rules written as SQL, incidents grouped over the topology graph, changes with their blast radius, and agents that show every query they ran. Ships with a simulated carrier network, so there is something to watch")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve the API and the view, for the command center (chloedb-server --app netops=URL) and for programs.
    Serve(ServeArgs),
}

#[derive(Args)]
struct ServeArgs {
    /// Where the API listens.
    #[arg(long, default_value = "127.0.0.1:7840")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer); chloedb-server sends its CHLOEDB_APP_TOKEN. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_NETOPS_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The chloedb-server (one server, or a cluster's coordinator) that keeps the tables.
    #[arg(long, env = "CHLOEDB_URL")]
    strata: Option<String>,
    /// Its API key (admin on the first start, which creates the netops_* tables; then one scoped to them).
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Start a chloedb-server on this machine, its database in this directory (again on the same directory: the same network).
    #[arg(long)]
    local: Option<PathBuf>,
    /// The chloedb-server binary for --local (default: next to this one, or on the PATH).
    #[arg(long, env = "CHLOEDB_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    /// Where the --local server listens (and its command center).
    #[arg(long, default_value = "127.0.0.1:7700")]
    local_listen: String,
    /// Hours of telemetry to write when the tables are first made.
    #[arg(long, env = "CHLOEDB_NETOPS_HISTORY_HOURS", default_value_t = 6)]
    history_hours: i64,
    /// Minutes between the faults the simulator injects on its own (0: only when asked).
    #[arg(long, env = "CHLOEDB_NETOPS_FAULT_EVERY", default_value_t = 45)]
    fault_every: i64,
    /// About how many objects the simulated estate behind the backbone has: hubs, cabinets, cell sites and what is behind them (0: none).
    #[arg(long, env = "CHLOEDB_NETOPS_ESTATE", default_value_t = 200_000)]
    estate: usize,
    /// The model that rewrites a search asked in a sentence into a plan, called through the server's llm() (empty: the fixed reader only).
    #[arg(long, env = "CHLOEDB_NETOPS_REWRITE_MODEL", default_value = "anthropic/claude-haiku-5-5")]
    rewrite_model: String,
    /// Do not write: serve the tables as they are.
    #[arg(long)]
    no_simulate: bool,
    /// Drop the netops_* tables first and seed them again.
    #[arg(long)]
    reset: bool,
}

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-netops: {m}");
    std::process::exit(1)
}

/// The chloedb-server binary: --server-bin, else next to this binary, else on the PATH.
fn server_bin(given: &Option<PathBuf>) -> PathBuf {
    if let Some(b) = given {
        return b.clone();
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        let p = dir.join("chloedb-server");
        if p.exists() {
            return p;
        }
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join("chloedb-server");
        if p.exists() {
            return p;
        }
    }
    fail("--local needs chloedb-server: give --server-bin, or put it on the PATH (cd chloedb-server && cargo build --release)")
}

fn random_token() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    } else {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        b = (t ^ (std::process::id() as u128) << 64).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A chloedb-server this process started (`--local`).
struct Local {
    child: Child,
    url: String,
}

impl Local {
    async fn start(dir: &PathBuf, bin: PathBuf, listen: &str, me: &str, token: &Option<String>) -> Result<Local, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("chloedb-server.log")).map_err(|e| format!("chloedb-server.log: {e}"))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&bin);
        cmd.args(["--db", &dir.join("netops.strata").to_string_lossy(), "--listen", listen, "--grpc", "off", "--app", &format!("netops={me}")]);
        if let Some(t) = token {
            cmd.env("CHLOEDB_APP_TOKEN", t).env("STRATA_APP_TOKEN", t);
        }
        cmd.stdout(Stdio::from(log)).stderr(Stdio::from(err)).stdin(Stdio::null()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("starting {}: {e}", bin.display()))?;
        let url = format!("http://{listen}");
        let s = Strata::new(&url, None);
        let t0 = Instant::now();
        loop {
            if s.health().await.is_ok() {
                return Ok(Local { child, url });
            }
            if let Ok(Some(code)) = child.try_wait() {
                return Err(format!("chloedb-server exited ({code}): see {}", dir.join("chloedb-server.log").display()));
            }
            if t0.elapsed() > Duration::from_secs(60) {
                return Err(format!("chloedb-server did not answer in a minute at {url}"));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// SIGTERM, then 10 s to exit.
    async fn stop(mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // SAFETY: a signal to a child this process started.
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        if tokio::time::timeout(Duration::from_secs(10), self.child.wait()).await.is_err() {
            let _ = self.child.kill().await;
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a signal handler");
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "chloedb_netops=info".into())).with_writer(std::io::stderr).with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr())).init();
    let Cmd::Serve(a) = Cli::parse().cmd;
    let token = a.token.clone().filter(|t| !t.is_empty()).or_else(|| a.local.as_ref().map(|_| random_token()));
    let listener = tokio::net::TcpListener::bind(&a.listen).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.listen)));
    let me = format!("http://{}", listener.local_addr().map(|x| x.to_string()).unwrap_or(a.listen.clone()));
    let (url, local) = match (&a.local, &a.strata) {
        (Some(_), Some(_)) => fail("give --strata or --local, not both"),
        (Some(dir), None) => {
            let l = Local::start(dir, server_bin(&a.server_bin), &a.local_listen, &me, &token).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-netops: a chloedb-server keeps the network in {} and answers at {} (open it: the app is in its command center)", dir.display(), l.url);
            (l.url.clone(), Some(l))
        }
        (None, url) => (url.clone().unwrap_or_else(|| "http://127.0.0.1:7700".into()), None),
    };
    let db = Strata::new(&url, a.key.clone());
    let opts = Opts { history_hours: a.history_hours.clamp(1, 48), fault_every: a.fault_every.clamp(0, 24 * 60), simulate: !a.no_simulate, reset: a.reset, estate: a.estate.min(2_000_000), rewrite_model: a.rewrite_model.clone() };
    let engine = if a.no_simulate {
        // The network is whatever the inventory tables say it is.
        let mut tries = 0;
        let w = loop {
            match Engine::inventory(&db).await {
                Ok(Some(w)) => break w,
                Ok(None) => fail("the simulator is off and netops_devices is empty. The netops_ tables now exist: load the inventory (netops_sites, netops_devices, netops_links, netops_services, netops_customers, netops_bgp) and start again, or start once without --no-simulate to see the app on a made-up network"),
                Err(e) if tries >= 30 => fail(format!("{url}: {e}")),
                Err(_) => { tries += 1; tokio::time::sleep(std::time::Duration::from_secs(1)).await; }
            }
        };
        eprintln!("chloedb-netops: the simulator is off; working from the inventory tables ({} devices, {} links)", w.devices.len(), w.links.len());
        Engine::with(db, opts, w)
    } else {
        Engine::new(db, opts)
    };
    tokio::spawn(engine.clone().run());
    let svc = Arc::new(Service { engine, token: token.clone(), local: local.is_some() });
    eprintln!("chloedb-netops: the API answers at {me}{}", if token.is_some() && a.token.is_none() { " (token made for the server)" } else { "" });
    if local.is_none() {
        eprintln!("chloedb-netops: to see it in the command center, start chloedb-server with --app netops={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN set to this token" } else { "" });
    }
    if let Err(e) = axum::serve(listener, api::router(svc)).with_graceful_shutdown(shutdown_signal()).await {
        eprintln!("chloedb-netops: {e}");
    }
    if let Some(l) = local {
        l.stop().await;
    }
}
