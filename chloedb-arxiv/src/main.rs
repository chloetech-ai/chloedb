//! `chloedb-arxiv`: arXiv on ChloeDB.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use tokio::process::{Child, Command};

use chloedb_arxiv::api::{self, Service};
use chloedb_arxiv::live::{self, Live};
use chloedb_arxiv::strata::Strata;
use chloedb_arxiv::{load, prepare, schema};

#[derive(Parser)]
#[command(name = "chloedb-arxiv", version, about = "arXiv on ChloeDB: every paper searched by words and meaning, with topics, trends, people, citations, what to read next and digests of your own")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Turn arXiv's metadata snapshot, OAI-PMH pages and Crossref's citations into the app's rows (JSON lines).
    Prepare(PrepareArgs),
    /// Load the rows `prepare` wrote into a chloedb-server (the tables are made first), then make the recommender.
    Load(LoadArgs),
    /// Serve the API, the view for the command center (chloedb-server --app arxiv=URL) and, with --public, the page itself.
    Serve(ServeArgs),
    /// Fetch the papers posted or revised since a day from arXiv's OAI-PMH interface and write them (once).
    Fetch(FetchArgs),
}

#[derive(Args)]
struct PrepareArgs {
    /// The snapshot's Parquet files (huggingface.co/datasets/librarian-bots/arxiv-metadata-snapshot).
    #[arg(long)]
    snapshot: PathBuf,
    /// OAI-PMH pages (arXivRaw) saved for the weeks after the snapshot.
    #[arg(long)]
    oai: Option<PathBuf>,
    /// Crossref's citations of arXiv papers (huggingface.co/datasets/cometadata/crossref-arxiv-citations, data/all.parquet).
    #[arg(long)]
    cites: Option<PathBuf>,
    /// Each paper's topic (id, a tab, topic), from tools/topics.py.
    #[arg(long)]
    topics: Option<PathBuf>,
    /// Where the rows go.
    #[arg(long)]
    out: PathBuf,
    /// Only papers whose primary category starts with one of these (cs., stat.ML).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
}

#[derive(Args)]
struct LoadArgs {
    /// The rows `prepare` wrote (and ax_topics.jsonl from tools/topics.py).
    #[arg(long)]
    rows: PathBuf,
    #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
    strata: String,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// The graph server (events, the recommender, citations, coauthors, what readers keep); by default --strata.
    #[arg(long, env = "CHLOEDB_ARXIV_GRAPH")]
    graph: Option<String>,
    #[arg(long, env = "CHLOEDB_ARXIV_GRAPH_KEY", hide_env_values = true)]
    graph_key: Option<String>,
    /// Only these tables (comma-separated).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// Megabytes of rows a request.
    #[arg(long, default_value_t = 32)]
    chunk_mb: usize,
    #[arg(long, default_value = schema::MODEL)]
    model: String,
    /// Do not make the recommender after loading (it counts every event in one transaction).
    #[arg(long)]
    no_recommender: bool,
}

#[derive(Args)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:7860")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer), but what --public lets anyone do; chloedb-server sends its CHLOEDB_APP_TOKEN. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_ARXIV_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The search server: the chloedb-server that keeps the papers, people and topics.
    #[arg(long, env = "CHLOEDB_URL")]
    strata: Option<String>,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// The graph server: the chloedb-server that keeps the reading events and the recommender, citations, coauthors and what readers keep, and calls the model (llm()). The recommender and graph walks need one database, so these tables are not sharded. By default the search server.
    #[arg(long, env = "CHLOEDB_ARXIV_GRAPH")]
    graph: Option<String>,
    #[arg(long, env = "CHLOEDB_ARXIV_GRAPH_KEY", hide_env_values = true)]
    graph_key: Option<String>,
    /// Start a chloedb-server on this machine, its database in this directory.
    #[arg(long)]
    local: Option<PathBuf>,
    /// With --local: start a second one for the graph, its database in this directory.
    #[arg(long)]
    local_graph: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:7701")]
    local_graph_listen: String,
    #[arg(long, env = "CHLOEDB_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:7700")]
    local_listen: String,
    /// Rows from `prepare` to load when the tables are empty.
    #[arg(long)]
    rows: Option<PathBuf>,
    #[arg(long, default_value = schema::MODEL)]
    model: String,
    /// The model of summaries, TL;DRs and digests, called through the server's llm() (it holds the provider's key).
    #[arg(long, env = "CHLOEDB_ARXIV_MODEL", default_value = "anthropic/claude-haiku-5-5")]
    answer_model: String,
    /// The model that explains each paper of a feed's issue from its full text (more text, read more carefully, than a TL;DR).
    #[arg(long, env = "CHLOEDB_ARXIV_ISSUE_MODEL", default_value = "anthropic/claude-sonnet-5-5")]
    issue_model: String,
    /// New model calls a day (kept summaries cost nothing).
    #[arg(long, default_value_t = 1000)]
    llm_per_day: usize,
    /// Serve the page at / and let anyone read, keep a library and feeds, within --per-minute requests a minute from one address.
    #[arg(long)]
    public: bool,
    #[arg(long, default_value_t = 240)]
    per_minute: usize,
    /// Keep the papers live from arXiv's OAI-PMH interface.
    #[arg(long)]
    live: bool,
    /// Hours between the live feed's rounds.
    #[arg(long, default_value_t = 6)]
    live_every_hours: u64,
}

#[derive(Args)]
struct FetchArgs {
    #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
    strata: String,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    #[arg(long, env = "CHLOEDB_ARXIV_GRAPH")]
    graph: Option<String>,
    #[arg(long, env = "CHLOEDB_ARXIV_GRAPH_KEY", hide_env_values = true)]
    graph_key: Option<String>,
    /// The first day (YYYY-MM-DD); by default the day the last round reached.
    #[arg(long)]
    from: Option<String>,
}

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-arxiv: {m}");
    std::process::exit(1)
}

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

struct Local {
    child: Child,
    url: String,
}

impl Local {
    async fn start(dir: &PathBuf, bin: PathBuf, listen: &str, me: Option<&str>, token: &Option<String>) -> Result<Local, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("chloedb-server.log")).map_err(|e| format!("chloedb-server.log: {e}"))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&bin);
        cmd.args(["--db", &dir.join("arxiv.chloedb").to_string_lossy(), "--listen", listen, "--grpc", "off"]);
        if let Some(me) = me {
            cmd.args(["--app", &format!("arxiv={me}")]);
            if let Some(t) = token {
                cmd.env("CHLOEDB_APP_TOKEN", t).env("STRATA_APP_TOKEN", t);
            }
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
            if t0.elapsed() > Duration::from_secs(300) {
                return Err(format!("chloedb-server did not answer in five minutes at {url}"));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn stop(mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // SAFETY: a signal to a child this process started.
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        if tokio::time::timeout(Duration::from_secs(30), self.child.wait()).await.is_err() {
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

async fn load_all(db: &Strata, graph: &Strata, rows: &PathBuf, only: &[String], chunk_mb: usize, recommender: bool) -> Result<(), String> {
    let n = load::load(db, graph, rows, only, chunk_mb, |m| eprintln!("chloedb-arxiv: {m}")).await?;
    eprintln!("chloedb-arxiv: loaded {n} rows");
    if recommender && (only.is_empty() || only.iter().any(|t| t == "ax_events")) {
        let t0 = Instant::now();
        eprintln!("chloedb-arxiv: making the recommender (it counts every event once)");
        match schema::ensure_recommender(graph).await {
            Ok(Some(v)) => eprintln!("chloedb-arxiv: recommender made in {:.0} s: {v}", t0.elapsed().as_secs_f64()),
            Ok(None) => eprintln!("chloedb-arxiv: the recommender was there"),
            Err(e) => return Err(format!("making the recommender: {e}")),
        }
    }
    Ok(())
}

async fn serve(a: ServeArgs) {
    let token = a.token.clone().filter(|t| !t.is_empty()).or_else(|| a.local.as_ref().map(|_| random_token()));
    let listener = tokio::net::TcpListener::bind(&a.listen).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.listen)));
    let me = format!("http://{}", listener.local_addr().map(|x| x.to_string()).unwrap_or(a.listen.clone()));
    let (url, local) = match (&a.local, &a.strata) {
        (Some(_), Some(_)) => fail("give --strata or --local, not both"),
        (Some(dir), None) => {
            let l = Local::start(dir, server_bin(&a.server_bin), &a.local_listen, Some(&me), &token).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-arxiv: a chloedb-server keeps arXiv in {} and answers at {} (its command center shows the app)", dir.display(), l.url);
            (l.url.clone(), Some(l))
        }
        (None, url) => (url.clone().unwrap_or_else(|| "http://127.0.0.1:7700".into()), None),
    };
    let db = Strata::new(&url, a.key.clone());
    let local_graph = match (&a.local_graph, &a.local) {
        (Some(dir), Some(_)) => {
            let l = Local::start(dir, server_bin(&a.server_bin), &a.local_graph_listen, None, &None).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-arxiv: a second chloedb-server keeps the graph in {} at {}", dir.display(), l.url);
            Some(l)
        }
        (Some(_), None) => fail("--local-graph goes with --local"),
        _ => None,
    };
    let graph = match (&local_graph, &a.graph) {
        (Some(l), _) => Strata::new(&l.url, None),
        (None, Some(g)) => Strata::new(g, a.graph_key.clone().or_else(|| a.key.clone())),
        (None, None) => db.clone(),
    };
    let made = async {
        let mut tries = 0;
        let made = loop {
            match schema::ensure(&db, &graph, &a.model).await {
                Ok(m) => break m,
                Err(e) if tries < 60 && e.status == 0 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => return Err(format!("{url}: making the tables: {e}")),
            }
        };
        if !made.is_empty() {
            eprintln!("chloedb-arxiv: made {}", made.join(", "));
        }
        if let Some(rows) = &a.rows {
            let n = db.query("SELECT COUNT(*) AS n FROM ax_papers", serde_json::Value::Null).await.ok().and_then(|r| r.first().and_then(|r| r.get("n").and_then(|v| v.as_i64()))).unwrap_or(0);
            if n == 0 {
                eprintln!("chloedb-arxiv: loading the rows in {}", rows.display());
                load_all(&db, &graph, rows, &[], 32, true).await?;
            }
        }
        Ok::<(), String>(())
    }
    .await;
    if let Err(m) = made {
        eprintln!("chloedb-arxiv: {m}");
        if let Some(l) = local {
            l.stop().await;
        }
        if let Some(l) = local_graph {
            l.stop().await;
        }
        std::process::exit(1)
    }
    let live = a.live.then(|| Arc::new(Live::default()));
    if let Some(l) = &live {
        tokio::spawn(live::run(db.clone(), graph.clone(), l.clone(), Duration::from_secs(a.live_every_hours.max(1) * 3600)));
    }
    let svc = Arc::new(Service {
        db,
        graph,
        token: token.clone(),
        public: a.public,
        model: a.answer_model.clone(),
        issue_model: a.issue_model.clone(),
        llm_per_day: a.llm_per_day,
        llm_used: Mutex::new((0, 0)),
        per_minute: a.per_minute.max(10),
        seen: Mutex::new(Default::default()),
        started: Instant::now(),
        live,
        pages: Default::default(),
        jobs: Mutex::new(Default::default()),
    });
    eprintln!("chloedb-arxiv: the API answers at {me}{}{}", if token.is_some() && a.token.is_none() { " (token made for the server)" } else { "" }, if a.public { ", and the page at / to anyone" } else { "" });
    if local.is_none() {
        eprintln!("chloedb-arxiv: to see it in the command center, start chloedb-server with --app arxiv={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN set to this token" } else { "" });
    }
    let routes = api::router(svc).into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, routes).with_graceful_shutdown(shutdown_signal()).await {
        eprintln!("chloedb-arxiv: {e}");
    }
    if let Some(l) = local {
        l.stop().await;
    }
    if let Some(l) = local_graph {
        l.stop().await;
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "chloedb_arxiv=info".into())).with_writer(std::io::stderr).with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr())).init();
    match Cli::parse().cmd {
        Cmd::Serve(a) => serve(a).await,
        Cmd::Prepare(a) => {
            let o = prepare::Opts { snapshot: a.snapshot, oai: a.oai, cites: a.cites, topics: a.topics, out: a.out, only: a.only, now: chrono::Utc::now().timestamp() };
            tokio::task::spawn_blocking(move || prepare::run(&o, |m| eprintln!("chloedb-arxiv: {m}"))).await.unwrap_or_else(|e| fail(e)).unwrap_or_else(|e| fail(e));
        }
        Cmd::Load(a) => {
            let db = Strata::new(&a.strata, a.key.clone());
            let graph = a.graph.as_ref().map(|g| Strata::new(g, a.graph_key.clone().or_else(|| a.key.clone()))).unwrap_or_else(|| db.clone());
            let made = schema::ensure(&db, &graph, &a.model).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.strata)));
            if !made.is_empty() {
                eprintln!("chloedb-arxiv: made {}", made.join(", "));
            }
            load_all(&db, &graph, &a.rows, &a.only, a.chunk_mb.clamp(1, 200), !a.no_recommender).await.unwrap_or_else(|e| fail(e));
        }
        Cmd::Fetch(a) => {
            let db = Strata::new(&a.strata, a.key.clone());
            let graph = a.graph.as_ref().map(|g| Strata::new(g, a.graph_key.clone().or_else(|| a.key.clone()))).unwrap_or_else(|| db.clone());
            let l = Live::default();
            let n = live::round(&db, &graph, &l, a.from.as_deref()).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-arxiv: wrote {n} papers: {}", l.status());
        }
    }
}
