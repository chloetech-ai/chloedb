//! `chloedb-hn`: Hacker News on ChloeDB.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use tokio::process::{Child, Command};

use chloedb_hn::api::{self, Service};
use chloedb_hn::ingest::Feed;
use chloedb_hn::strata::Strata;
use chloedb_hn::{load, prepare, schema};

#[derive(Parser)]
#[command(name = "chloedb-hn", version, about = "Hacker News on ChloeDB: every story and comment searched by words and meaning, with threads, trends, experts, the reply graph and cited answers")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Turn the archive's monthly Parquet files into the app's rows (JSON lines), downloading the months missing first.
    Prepare(PrepareArgs),
    /// Load the rows `prepare` wrote into a chloedb-server (the tables are made first).
    Load(LoadArgs),
    /// Serve the API, the view for the command center (chloedb-server --app hn=URL) and, with --public, the page itself.
    Serve(ServeArgs),
    /// Write autocomplete's rows (hn_complete.jsonl: phrases of titles, sites, topics, people) from the rows `prepare` wrote.
    Complete(CompleteArgs),
}

#[derive(clap::Args)]
struct CompleteArgs {
    /// Where `prepare` wrote the rows; hn_complete.jsonl is written there too.
    #[arg(long)]
    rows: PathBuf,
}

#[derive(Args)]
struct LoadArgs {
    /// The rows: hn_users.jsonl, hn_stories.jsonl, hn_comments.jsonl, hn_replies.jsonl, hn_links.jsonl.
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
    #[arg(long, default_value_t = 32)]
    chunk_mb: usize,
    /// The embedding model of the tables, when they are made.
    #[arg(long, default_value = schema::MODEL)]
    model: String,
    /// The comments' embedding model (`none`: no vectors for comments, which then take a third of the bytes).
    #[arg(long, default_value = schema::MODEL)]
    comment_model: String,
}

#[derive(Args)]
struct ServeArgs {
    /// Where the API listens.
    #[arg(long, default_value = "127.0.0.1:7850")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer), but reads with --public; chloedb-server sends its CHLOEDB_APP_TOKEN. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_HN_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The chloedb-server (one server, or a cluster's coordinator) that keeps the tables.
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
    /// Rows from `prepare` to load when the tables are empty.
    #[arg(long)]
    rows: Option<PathBuf>,
    /// The embedding model of titles and comments.
    #[arg(long, default_value = schema::MODEL)]
    model: String,
    /// The comments' embedding model (`none`: no vectors for comments).
    #[arg(long, default_value = schema::MODEL)]
    comment_model: String,
    /// The model behind "What HN thinks", called through the server's llm() (it holds the provider's key).
    #[arg(long, env = "CHLOEDB_HN_ANSWER_MODEL", default_value = "anthropic/claude-haiku-5-5")]
    answer_model: String,
    /// New answers a day (answers asked before are kept and cost nothing).
    #[arg(long, default_value_t = 300)]
    ask_per_day: usize,
    /// Serve the page at / and let anyone read, without the token, within --per-minute requests a minute from one address.
    #[arg(long)]
    public: bool,
    #[arg(long, default_value_t = 120)]
    per_minute: usize,
    /// Keep the tables live from the HN API: new items as they are posted, and the points and comments of the stories that change.
    #[arg(long)]
    feed: bool,
    /// Seconds between the feed's rounds.
    #[arg(long, default_value_t = 30)]
    feed_every: u64,
    /// The feed's requests in flight to the HN API.
    #[arg(long, default_value_t = 8)]
    feed_concurrency: usize,
}

#[derive(Args)]
struct PrepareArgs {
    /// Where the monthly Parquet files are (and are downloaded to).
    #[arg(long)]
    parquet: PathBuf,
    /// Where the rows go: hn_stories.jsonl, hn_comments.jsonl, hn_users.jsonl, hn_replies.jsonl, hn_links.jsonl.
    #[arg(long)]
    out: PathBuf,
    /// Download the months from --from to --to that are not in --parquet yet.
    #[arg(long)]
    download: bool,
    #[arg(long, default_value = "2006-10")]
    from: String,
    #[arg(long, default_value = "2026-08")]
    to: String,
    /// Write only the comments from this date on (every story is written).
    #[arg(long)]
    comments_since: Option<String>,
    /// And before this date.
    #[arg(long)]
    comments_until: Option<String>,
    /// Leave pairs of people who replied fewer times than this out of the reply graph.
    #[arg(long, default_value_t = 1)]
    min_replies: u32,
    /// Only download.
    #[arg(long)]
    download_only: bool,
}

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-hn: {m}");
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
        cmd.args(["--db", &dir.join("hn.chloedb").to_string_lossy(), "--listen", listen, "--grpc", "off", "--app", &format!("hn={me}")]);
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
            eprintln!("chloedb-hn: a chloedb-server keeps Hacker News in {} and answers at {} (open it: the app is in its command center)", dir.display(), l.url);
            (l.url.clone(), Some(l))
        }
        (None, url) => (url.clone().unwrap_or_else(|| "http://127.0.0.1:7700".into()), None),
    };
    let db = Strata::new(&url, a.key.clone());
    let made = async {
        let mut tries = 0;
        let made = loop {
            match schema::ensure(&db, &a.model, &a.comment_model).await {
                Ok(m) => break m,
                Err(e) if tries < 30 && e.status == 0 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => return Err(format!("{url}: making the tables: {e}")),
            }
        };
        if !made.is_empty() {
            eprintln!("chloedb-hn: made {}", made.join(", "));
        }
        if let Some(rows) = &a.rows {
            let n = db.query("SELECT COUNT(*) AS n FROM hn_stories", serde_json::Value::Null).await.ok().and_then(|r| r.first().and_then(|r| r.get("n").and_then(|v| v.as_i64()))).unwrap_or(0);
            if n == 0 {
                eprintln!("chloedb-hn: loading the rows in {}", rows.display());
                let n = load::load(&db, rows, &[], 32, |m| eprintln!("chloedb-hn: {m}")).await?;
                eprintln!("chloedb-hn: loaded {n} rows");
            }
        }
        Ok::<(), String>(())
    }
    .await;
    if let Err(m) = made {
        eprintln!("chloedb-hn: {m}");
        if let Some(l) = local {
            l.stop().await;
        }
        std::process::exit(1)
    }
    let feed = a.feed.then(|| Feed::new(db.clone(), a.feed_concurrency));
    if let Some(f) = &feed {
        tokio::spawn(f.clone().run(Duration::from_secs(a.feed_every.max(5))));
        tokio::spawn(f.clone().backfill());
    }
    let svc = Arc::new(Service {
        db,
        feed,
        token: token.clone(),
        public: a.public,
        model: a.answer_model.clone(),
        ask_per_day: a.ask_per_day,
        asks: Mutex::new((0, 0)),
        per_minute: a.per_minute.max(10),
        seen: Mutex::new(Default::default()),
        started: Instant::now(),
    });
    eprintln!("chloedb-hn: the API answers at {me}{}{}", if token.is_some() && a.token.is_none() { " (token made for the server)" } else { "" }, if a.public { ", and the page at / to anyone" } else { "" });
    if local.is_none() {
        eprintln!("chloedb-hn: to see it in the command center, start chloedb-server with --app hn={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN set to this token" } else { "" });
    }
    let routes = api::router(svc).into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, routes).with_graceful_shutdown(shutdown_signal()).await {
        eprintln!("chloedb-hn: {e}");
    }
    if let Some(l) = local {
        l.stop().await;
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "chloedb_hn=info".into())).with_writer(std::io::stderr).with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr())).init();
    match Cli::parse().cmd {
        Cmd::Serve(a) => serve(a).await,
        Cmd::Complete(a) => {
            let t0 = std::time::Instant::now();
            let n = chloedb_hn::complete::build(&a.rows, |m| eprintln!("chloedb-hn: {m}")).unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-hn: wrote {n} rows to {} in {:.0} s", a.rows.join("hn_complete.jsonl").display(), t0.elapsed().as_secs_f64());
        }
        Cmd::Load(a) => {
            let db = Strata::new(&a.strata, a.key.clone());
            let made = schema::ensure(&db, &a.model, &a.comment_model).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.strata)));
            if !made.is_empty() {
                eprintln!("chloedb-hn: made {}", made.join(", "));
            }
            let n = load::load(&db, &a.rows, &a.only, a.chunk_mb.clamp(1, 200), |m| eprintln!("chloedb-hn: {m}")).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-hn: loaded {n} rows");
        }
        Cmd::Prepare(a) => {
            if a.download || a.download_only {
                prepare::download(&a.parquet, &prepare::months(&a.from, &a.to)).await.unwrap_or_else(|e| fail(e));
            }
            if a.download_only {
                return;
            }
            let o = prepare::Opts { parquet: a.parquet, out: a.out, comments_since: a.comments_since, comments_until: a.comments_until, min_replies: a.min_replies };
            tokio::task::spawn_blocking(move || prepare::run(&o)).await.unwrap_or_else(|e| fail(e)).unwrap_or_else(|e| fail(e));
        }
    }
}
