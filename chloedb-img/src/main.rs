//! `chloedb-img`: Image Explorer on ChloeDB.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use tokio::process::{Child, Command};

use chloedb_img::api::{self, Service};
use chloedb_img::clip::Clip;
use chloedb_img::strata::Strata;
use chloedb_img::thumbs::Thumbs;
use chloedb_img::{load, prepare};

#[derive(Parser)]
#[command(name = "chloedb-img", version, about = "Image Explorer on ChloeDB: 12.4 million free pictures searched by what is in them")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Make each picture's kind and theme, and the themes, from PD12M's vectors.
    Prepare(PrepareArgs),
    /// Load PD12M's records and vectors, with what prepare made, into the server.
    Load(LoadArgs),
    /// Serve the API, the view for the command center (chloedb-server --app img=URL) and, with --public, the page itself.
    Serve(ServeArgs),
}

#[derive(Args)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:7870")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer), but what --public lets anyone do; chloedb-server sends its CHLOEDB_APP_TOKEN. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_IMG_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// The chloedb-server that keeps the pictures.
    #[arg(long, env = "CHLOEDB_URL")]
    strata: Option<String>,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Start a chloedb-server on this machine, its database in this directory (load into it with `load`).
    #[arg(long)]
    local: Option<PathBuf>,
    #[arg(long, env = "CHLOEDB_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:7700")]
    local_listen: String,
    /// Where the CLIP model is (clip-vit-large-patch14/ under it).
    #[arg(long, env = "CHLOEDB_MODELS")]
    models: Option<PathBuf>,
    /// Words and photos embedded at once.
    #[arg(long, default_value_t = 2)]
    embed_slots: usize,
    /// Where thumbnails are kept.
    #[arg(long, env = "CHLOEDB_IMG_CACHE")]
    cache: PathBuf,
    /// Originals fetched at once to make thumbnails.
    #[arg(long, default_value_t = 16)]
    fetch_slots: usize,
    /// Look the pages of pictures from Wikimedia Commons up there (by the file's SHA-1).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    commons: bool,
    /// Serve the page at / and let anyone search, within the limits below a minute from one address.
    #[arg(long)]
    public: bool,
    #[arg(long, default_value_t = 240)]
    per_minute: usize,
    /// Photos and details embedded a minute from one address.
    #[arg(long, default_value_t = 20)]
    photos_per_minute: usize,
    /// New thumbnails made a minute for one address (thumbnails made before are not counted).
    #[arg(long, default_value_t = 600)]
    thumbs_per_minute: usize,
    /// At start, make the thumbnails of this many pictures nearest each theme's middle.
    #[arg(long, default_value_t = 9)]
    warm_per_theme: usize,
    /// At start, also make the thumbnails (and so the colors) of the first this many pictures.
    #[arg(long, default_value_t = 0)]
    warm_first: usize,
}

#[derive(Args)]
struct PrepareArgs {
    /// The folder with PD12M's files (pd12m.000.parquet ..., pd12m.00.npy ...).
    #[arg(long)]
    data: PathBuf,
    /// The first this many pictures.
    #[arg(long, default_value_t = 12_400_000)]
    rows: usize,
    /// Where the CLIP model is (clip-vit-large-patch14/ under it).
    #[arg(long, env = "CHLOEDB_MODELS")]
    models: Option<PathBuf>,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 42)]
    themes: usize,
    #[arg(long, default_value_t = 200_000)]
    sample: usize,
    #[arg(long, default_value_t = 20)]
    iterations: usize,
    /// Names chosen for the themes (data/theme-names-1m.json names those of the first million).
    #[arg(long)]
    names: Option<PathBuf>,
    /// Keep the themes of an earlier prepare (its themes.json): their centres, names and places on the map, with every picture placed in the nearest, so that a larger slice keeps the map of a smaller one.
    #[arg(long)]
    themes_from: Option<PathBuf>,
}

#[derive(Args)]
struct LoadArgs {
    #[arg(long)]
    data: PathBuf,
    /// What prepare wrote.
    #[arg(long)]
    prepared: PathBuf,
    #[arg(long, default_value_t = 12_400_000)]
    rows: usize,
    #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
    strata: String,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    #[arg(long, default_value_t = 4000)]
    chunk: usize,
    /// Go on from this picture.
    #[arg(long, default_value_t = 0)]
    from: usize,
}

fn models_dir(m: Option<PathBuf>) -> PathBuf {
    m.unwrap_or_else(chloedb::embed::models_dir)
}

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-img: {m}");
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

/// A chloedb-server this app starts, with itself as its app `img`.
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
        cmd.args(["--db", &dir.join("img.chloedb").to_string_lossy(), "--listen", listen, "--grpc", "off", "--app", &format!("img={me}")]);
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

async fn serve(a: ServeArgs) -> Result<(), String> {
    let token = a.token.clone().filter(|t| !t.is_empty()).or_else(|| a.local.as_ref().map(|_| chloedb_img::random_token()));
    let listener = tokio::net::TcpListener::bind(&a.listen).await.map_err(|e| format!("{}: {e}", a.listen))?;
    let me = format!("http://{}", listener.local_addr().map(|x| x.to_string()).unwrap_or(a.listen.clone()));
    let (url, local) = match (&a.local, &a.strata) {
        (Some(_), Some(_)) => return Err("give --strata or --local, not both".into()),
        (Some(dir), None) => {
            let l = Local::start(dir, server_bin(&a.server_bin), &a.local_listen, &me, &token).await?;
            eprintln!("chloedb-img: a chloedb-server keeps the pictures in {} and answers at {} (its command center shows the app)", dir.display(), l.url);
            (l.url.clone(), Some(l))
        }
        (None, url) => (url.clone().unwrap_or_else(|| "http://127.0.0.1:7700".into()), None),
    };
    let db = Strata::new(&url, a.key.clone());
    let mut tries = 0;
    loop {
        match chloedb_img::schema::create(&db).await {
            Ok(()) => break,
            Err(e) if tries < 60 && e.status == 0 => {
                tries += 1;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(format!("{url}: making the tables: {e}")),
        }
    }
    let models = models_dir(a.models.clone());
    let t0 = Instant::now();
    let clip = Arc::new(tokio::task::spawn_blocking(move || Clip::open(&models, a.embed_slots)).await.map_err(|e| e.to_string())??);
    eprintln!("chloedb-img: {} opened in {:.1} s", clip.name(), t0.elapsed().as_secs_f64());
    std::fs::create_dir_all(&a.cache).map_err(|e| format!("{}: {e}", a.cache.display()))?;
    let thumbs = Arc::new(Thumbs::new(&a.cache, db.clone(), a.fetch_slots, a.commons));
    let svc = Arc::new(Service::new(db.clone(), clip, thumbs, token.clone(), a.public, a.per_minute.max(10), a.photos_per_minute.max(1), a.thumbs_per_minute.max(10)));
    if let Ok(r) = db.query("SELECT COUNT(*) AS n FROM img_pictures", serde_json::Value::Null).await {
        let n = r.first().and_then(|r| r.get("n")).and_then(|v| v.as_i64()).unwrap_or(0);
        svc.total.store(n as u64, std::sync::atomic::Ordering::Relaxed);
        eprintln!("chloedb-img: {n} pictures in {url}");
    }
    if a.warm_per_theme > 0 || a.warm_first > 0 {
        tokio::spawn(api::warm(svc.clone(), a.warm_per_theme, a.warm_first, a.fetch_slots));
    }
    eprintln!("chloedb-img: the API answers at {me}{}{}", if token.is_some() && a.token.is_none() { " (token made for the server)" } else { "" }, if a.public { ", and the page at / to anyone" } else { "" });
    if local.is_none() {
        eprintln!("chloedb-img: to see it in the command center, start chloedb-server with --app img={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN set to this token" } else { "" });
    }
    let routes = api::router(svc).into_make_service_with_connect_info::<std::net::SocketAddr>();
    let r = axum::serve(listener, routes).with_graceful_shutdown(shutdown_signal()).await.map_err(|e| e.to_string());
    if let Some(l) = local {
        l.stop().await;
    }
    r
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).with_writer(std::io::stderr).init();
    let cli = Cli::parse();
    let r = match cli.cmd {
        Cmd::Prepare(a) => {
            let o = prepare::Options { data: a.data, rows: a.rows, models: models_dir(a.models), out: a.out, themes: a.themes, sample: a.sample, iterations: a.iterations, names: a.names, themes_from: a.themes_from };
            tokio::task::spawn_blocking(move || prepare::run(&o)).await.map_err(|e| e.to_string()).and_then(|r| r)
        }
        Cmd::Load(a) => {
            let db = Strata::new(&a.strata, a.key);
            load::run(&db, &load::Options { data: a.data, prepared: a.prepared, rows: a.rows, chunk: a.chunk, from: a.from }).await
        }
        Cmd::Serve(a) => serve(a).await,
    };
    if let Err(e) = r {
        eprintln!("chloedb-img: {e}");
        std::process::exit(1);
    }
}
