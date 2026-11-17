//! `chloedb-sound`: Sound Explorer on ChloeDB.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use tokio::process::{Child, Command};

use chloedb_sound::api::{self, Service};
use chloedb_sound::strata::Strata;
use chloedb_sound::vecs::Embedder;
use chloedb_sound::{load, schema};

#[derive(Parser)]
#[command(name = "chloedb-sound", version, about = "Sound Explorer on ChloeDB: free sounds and music searched by words and by how they sound, inside long recordings too")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Make the tables (and the labels, which the server embeds) on a chloedb-server, for tools/prepare.py to embed against.
    Init(InitArgs),
    /// Load the rows tools/prepare.py wrote (au_sounds.jsonl, au_moments.jsonl) into a chloedb-server.
    Load(LoadArgs),
    /// Serve the API, the view for the command center (chloedb-server --app sound=URL) and, with --public, the page itself.
    Serve(ServeArgs),
}

#[derive(Args)]
struct InitArgs {
    #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
    strata: String,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
}

#[derive(Args)]
struct LoadArgs {
    /// Where tools/prepare.py wrote the rows.
    #[arg(long)]
    rows: PathBuf,
    #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
    strata: String,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Only these tables (comma-separated).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// Megabytes of rows a request.
    #[arg(long, default_value_t = 16)]
    chunk_mb: usize,
}

#[derive(Args)]
struct ServeArgs {
    /// Where the API listens.
    #[arg(long, default_value = "127.0.0.1:7870")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer), but reads with --public; chloedb-server sends its CHLOEDB_APP_TOKEN. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_SOUND_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The chloedb-server (one server, or a cluster's coordinator) that keeps the tables; it needs CLAP (built with --features audio).
    #[arg(long, env = "CHLOEDB_URL")]
    strata: Option<String>,
    /// Its API key.
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Start a chloedb-server on this machine, its database in this directory.
    #[arg(long)]
    local: Option<PathBuf>,
    /// The chloedb-server binary for --local (default: next to this one, or on the PATH).
    #[arg(long, env = "CHLOEDB_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    /// Where the --local server listens (and its command center).
    #[arg(long, default_value = "127.0.0.1:7700")]
    local_listen: String,
    /// Rows from tools/prepare.py to load when the tables are empty; their previews are in its audio/ folder.
    #[arg(long)]
    rows: Option<PathBuf>,
    /// Where the sounds' previews are (default: the audio/ folder of --rows).
    #[arg(long)]
    audio: Option<PathBuf>,
    /// Serve the page at / and let anyone read, without the token, within --per-minute requests a minute from one address.
    #[arg(long)]
    public: bool,
    #[arg(long, default_value_t = 120)]
    per_minute: usize,
    /// Clips heard a minute from one address, in public.
    #[arg(long, default_value_t = 6)]
    listens_per_minute: usize,
}

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-sound: {m}");
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
        cmd.args(["--db", &dir.join("sound.chloedb").to_string_lossy(), "--listen", listen, "--grpc", "off", "--app", &format!("sound={me}")]);
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
            if t0.elapsed() > Duration::from_secs(120) {
                return Err(format!("chloedb-server did not answer in two minutes at {url}"));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// SIGTERM, then 20 s to exit.
    async fn stop(mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // SAFETY: a signal to a child this process started.
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        if tokio::time::timeout(Duration::from_secs(20), self.child.wait()).await.is_err() {
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

async fn serve(a: ServeArgs) {
    let token = a.token.clone().filter(|t| !t.is_empty()).or_else(|| a.local.as_ref().map(|_| random_token()));
    let listener = tokio::net::TcpListener::bind(&a.listen).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.listen)));
    let me = format!("http://{}", listener.local_addr().map(|x| x.to_string()).unwrap_or(a.listen.clone()));
    let (url, local) = match (&a.local, &a.strata) {
        (Some(_), Some(_)) => fail("give --strata or --local, not both"),
        (Some(dir), None) => {
            let l = Local::start(dir, server_bin(&a.server_bin), &a.local_listen, &me, &token).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-sound: a chloedb-server keeps the sounds in {} and answers at {} (open it: the app is in its command center)", dir.display(), l.url);
            (l.url.clone(), Some(l))
        }
        (None, url) => (url.clone().unwrap_or_else(|| "http://127.0.0.1:7700".into()), None),
    };
    let db = Strata::new(&url, a.key.clone());
    let made = async {
        let mut tries = 0;
        let made = loop {
            match schema::ensure(&db).await {
                Ok(m) => break m,
                Err(e) if tries < 30 && e.status == 0 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => return Err(format!("{url}: making the tables: {e}")),
            }
        };
        if !made.is_empty() {
            eprintln!("chloedb-sound: made {}", made.join(", "));
        }
        if let Some(rows) = &a.rows {
            let n = db.query("SELECT COUNT(*) AS n FROM au_sounds", serde_json::Value::Null).await.ok().and_then(|r| r.first().and_then(|r| r.get("n").and_then(|v| v.as_i64()))).unwrap_or(0);
            if n == 0 {
                eprintln!("chloedb-sound: loading the rows in {}", rows.display());
                let n = load::load(&db, rows, &[], 16, |m| eprintln!("chloedb-sound: {m}")).await?;
                eprintln!("chloedb-sound: loaded {n} rows");
            }
        }
        Ok::<(), String>(())
    }
    .await;
    if let Err(m) = made {
        eprintln!("chloedb-sound: {m}");
        if let Some(l) = local {
            l.stop().await;
        }
        std::process::exit(1)
    }
    let audio = a.audio.clone().or_else(|| a.rows.as_ref().map(|r| r.join("audio"))).unwrap_or_else(|| PathBuf::from("audio"));
    let svc = Arc::new(Service {
        emb: Arc::new(Embedder::new(db.clone())),
        db,
        audio,
        token: token.clone(),
        public: a.public,
        per_minute: a.per_minute.max(10),
        listens_per_minute: a.listens_per_minute.max(1),
        seen: Mutex::new(Default::default()),
        files: Mutex::new(Default::default()),
        map: Mutex::new(None),
        started: Instant::now(),
    });
    eprintln!("chloedb-sound: the API answers at {me}{}{}", if token.is_some() && a.token.is_none() { " (token made for the server)" } else { "" }, if a.public { ", and the page at / to anyone" } else { "" });
    if local.is_none() {
        eprintln!("chloedb-sound: to see it in the command center, start chloedb-server with --app sound={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN set to this token" } else { "" });
    }
    let routes = api::router(svc).into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, routes).with_graceful_shutdown(shutdown_signal()).await {
        eprintln!("chloedb-sound: {e}");
    }
    if let Some(l) = local {
        l.stop().await;
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "chloedb_sound=info".into())).with_writer(std::io::stderr).with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr())).init();
    match Cli::parse().cmd {
        Cmd::Serve(a) => serve(a).await,
        Cmd::Init(a) => {
            let db = Strata::new(&a.strata, a.key.clone());
            let made = schema::ensure(&db).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.strata)));
            eprintln!("chloedb-sound: {}", if made.is_empty() { "the tables were there".to_string() } else { format!("made {}", made.join(", ")) });
        }
        Cmd::Load(a) => {
            let db = Strata::new(&a.strata, a.key.clone());
            let made = schema::ensure(&db).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.strata)));
            if !made.is_empty() {
                eprintln!("chloedb-sound: made {}", made.join(", "));
            }
            let n = load::load(&db, &a.rows, &a.only, a.chunk_mb.clamp(1, 200), |m| eprintln!("chloedb-sound: {m}")).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-sound: loaded {n} rows");
        }
    }
}
