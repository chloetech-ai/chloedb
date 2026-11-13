//! `chloedb-sheets serve`: the spreadsheets' API and view, for ChloeDB's
//! command center; `chloedb-sheets import FILE` and `export ID` for scripts.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use serde_json::json;
use tokio::process::{Child, Command};

use chloedb_sheets::api::{self, Service};
use chloedb_sheets::books::Books;
use chloedb_sheets::strata::Strata;

#[derive(Parser)]
#[command(name = "chloedb-sheets", version, about = "Spreadsheets kept in ChloeDB: an app of its command center, with formulas, charts, pivots, xlsx, CSV and SQLite in and out, and the cluster's tables as sheets")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve the API and the view, for the command center (chloedb-server --app sheets=URL) and for programs.
    Serve(ServeArgs),
    /// Import a file (xlsx, CSV, SQLite) as a new workbook, and print its id.
    Import(ImportArgs),
    /// Write a workbook as an xlsx file (or a sheet of it as CSV).
    Export(ExportArgs),
}

#[derive(Args)]
struct Target {
    /// The chloedb-server (one server, or a cluster's coordinator) that keeps the workbooks.
    #[arg(long, env = "CHLOEDB_URL")]
    strata: Option<String>,
    /// Its API key (admin: the first start creates the two tables; then write).
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Start a chloedb-server on this machine, its database in this directory (again on the same directory: the same workbooks).
    #[arg(long)]
    local: Option<PathBuf>,
    /// The chloedb-server binary for --local (default: next to this one, or on the PATH).
    #[arg(long, env = "CHLOEDB_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    /// Where the --local server listens (and its command center).
    #[arg(long, default_value = "127.0.0.1:7700")]
    local_listen: String,
}

#[derive(Args)]
struct ServeArgs {
    /// Where the API listens.
    #[arg(long, default_value = "127.0.0.1:7820")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer); chloedb-server sends its CHLOEDB_APP_TOKEN. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_SHEETS_TOKEN", hide_env_values = true)]
    token: Option<String>,
    #[command(flatten)]
    target: Target,
}

#[derive(Args)]
struct ImportArgs {
    file: PathBuf,
    /// The workbook's name (default: the file's).
    #[arg(long)]
    name: Option<String>,
    /// Of a SQLite file, the tables to bring (comma-separated; default all).
    #[arg(long)]
    tables: Option<String>,
    #[command(flatten)]
    target: Target,
}

#[derive(Args)]
struct ExportArgs {
    id: String,
    /// The file to write (.xlsx, or .csv for one sheet).
    #[arg(long, short)]
    out: PathBuf,
    /// For CSV, the sheet (its index, from 0).
    #[arg(long, default_value_t = 0)]
    sheet: u32,
    #[command(flatten)]
    target: Target,
}

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-sheets: {m}");
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
    async fn start(dir: &PathBuf, bin: PathBuf, listen: &str, app: Option<(String, Option<String>)>) -> Result<Local, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("chloedb-server.log")).map_err(|e| format!("chloedb-server.log: {e}"))?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&bin);
        cmd.args(["--db", &dir.join("sheets.strata").to_string_lossy(), "--listen", listen, "--grpc", "off"]);
        // Sheets is this server's app: it starts none of its own.
        cmd.env("CHLOEDB_START_APPS", "off");
        if let Some((url, token)) = &app {
            cmd.args(["--app", &format!("sheets={url}")]);
            if let Some(t) = token {
                // Under both names: the binary may be one from before the
                // rename, which reads only STRATA_APP_TOKEN.
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

async fn connect(t: &Target, app: Option<(String, Option<String>)>) -> (Strata, Option<Local>) {
    let (url, local) = match (&t.local, &t.strata) {
        (Some(_), Some(_)) => fail("give --strata or --local, not both"),
        (Some(dir), None) => {
            let l = Local::start(dir, server_bin(&t.server_bin), &t.local_listen, app).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-sheets: a chloedb-server keeps the workbooks in {} and answers at {} (its command center too)", dir.display(), l.url);
            (l.url.clone(), Some(l))
        }
        (None, url) => (url.clone().unwrap_or_else(|| "http://127.0.0.1:7700".into()), None),
    };
    let strata = Strata::new(&url, t.key.clone());
    if local.is_none() {
        if let Err(e) = strata.health().await {
            eprintln!("chloedb-sheets: warning: {url} does not answer yet ({e})");
        }
    }
    (strata, local)
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

fn main() {
    // Settings may be named CHLOEDB_* (the name since the rename) or STRATA_*:
    // clap and the code read the old names, so the new ones are copied over
    // before the runtime starts any thread.
    chloedb_sheets::adopt_old_env_names();
    run();
}

#[tokio::main]
async fn run() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "chloedb_sheets=info".into())).with_writer(std::io::stderr).with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr())).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve(a) => {
            let token = a.token.clone().or_else(|| a.target.local.as_ref().map(|_| random_token()));
            // Started by chloedb-server: the socket it holds, not --listen.
            let listener = match chloedb_sheets::inherited_listener().unwrap_or_else(|e| fail(e)) {
                Some(l) => l.set_nonblocking(true).and_then(|()| tokio::net::TcpListener::from_std(l)).unwrap_or_else(|e| fail(format!("the inherited socket: {e}"))),
                None => tokio::net::TcpListener::bind(&a.listen).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.listen))),
            };
            let me = format!("http://{}", listener.local_addr().map(|x| x.to_string()).unwrap_or(a.listen.clone()));
            let (strata, local) = connect(&a.target, Some((me.clone(), token.clone()))).await;
            let books = Books::new(strata);
            if let Err(e) = books.ready().await {
                eprintln!("chloedb-sheets: warning: {} (tried again on the first request)", e.message);
            }
            tokio::spawn(books.clone().keeper());
            let svc = Arc::new(Service { books: books.clone(), token: token.clone(), local: local.is_some() });
            eprintln!("chloedb-sheets: the API answers at {me}{}", if token.is_some() && a.token.is_none() { " (token made for the server)" } else { "" });
            // A chloedb-server that started it (CHLOEDB_APP_OF) shows it already.
            if local.is_none() && std::env::var_os("CHLOEDB_APP_OF").is_none() {
                eprintln!("chloedb-sheets: to see it in the command center, start chloedb-server with --app sheets={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN set to this token" } else { "" });
            }
            let server = axum::serve(listener, api::router(svc)).with_graceful_shutdown(shutdown_signal());
            if let Err(e) = server.await {
                eprintln!("chloedb-sheets: {e}");
            }
            books.flush().await;
            if let Some(l) = local {
                l.stop().await;
            }
        }
        Cmd::Import(a) => {
            let bytes = std::fs::read(&a.file).unwrap_or_else(|e| fail(format!("{}: {e}", a.file.display())));
            let fname = a.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into());
            let name = a.name.clone().unwrap_or(fname.clone());
            let (strata, local) = connect(&a.target, None).await;
            let books = Books::new(strata);
            let tables: Option<Vec<String>> = a.tables.as_deref().map(|t| t.split(',').map(|s| s.trim().to_string()).collect());
            let format = chloedb_sheets::io::sniff(&bytes, &fname);
            let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name.clone());
            let r = tokio::task::block_in_place(|| match format {
                "xlsx" => chloedb_sheets::io::from_xlsx(&bytes, &stem).map(|e| (e, json!({"format": "xlsx"}))),
                "sqlite" => chloedb_sheets::io::from_sqlite(&bytes, &name, tables.as_deref()),
                _ => chloedb_sheets::io::from_csv(&bytes, &fname, None),
            });
            let code = match r {
                Ok((engine, report)) => match books.create(engine, &format!("{format}:{fname}"), Default::default()).await {
                    Ok(s) => {
                        println!("{}", serde_json::to_string(&json!({"id": s["id"], "name": s["name"], "sheets": s["sheets"].as_array().map(|a| a.len()), "import": report})).expect("json"));
                        0
                    }
                    Err(e) => {
                        eprintln!("chloedb-sheets: {}", e.message);
                        1
                    }
                },
                Err(e) => {
                    eprintln!("chloedb-sheets: {e}");
                    1
                }
            };
            if let Some(l) = local {
                l.stop().await;
            }
            std::process::exit(code);
        }
        Cmd::Export(a) => {
            let (strata, local) = connect(&a.target, None).await;
            let books = Books::new(strata);
            let code = match books.open(&a.id).await {
                Ok(g) => {
                    let b = g.as_ref().expect("open");
                    let csv = a.out.extension().is_some_and(|e| e.eq_ignore_ascii_case("csv"));
                    let bytes = if csv { chloedb_sheets::io::to_csv(b.model(), a.sheet).map(String::into_bytes) } else { chloedb_sheets::io::to_xlsx(b.model()) };
                    match bytes.and_then(|x| std::fs::write(&a.out, x).map_err(|e| e.to_string())) {
                        Ok(()) => {
                            eprintln!("chloedb-sheets: wrote {}", a.out.display());
                            0
                        }
                        Err(e) => {
                            eprintln!("chloedb-sheets: {e}");
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("chloedb-sheets: {}", e.message);
                    1
                }
            };
            if let Some(l) = local {
                l.stop().await;
            }
            std::process::exit(code);
        }
    }
}
