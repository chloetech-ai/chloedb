//! `chloedb-experiments check`: every off-policy estimator against the
//! simulator's truth, on BEIR collections as sites, under six click models
//! and three logging policies. `chloedb-experiments sim`: one simulated
//! run, served (each list a logged, exploring query to chloedb-server) or
//! fast (the same lists drawn here), checked impression by impression.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};
use tokio::process::{Child, Command};

use chloedb_experiments::check::{self, Fetched};
use chloedb_experiments::click::Model;
use chloedb_experiments::estimate::{self, Counts, Models, Target, Valuer};
use chloedb_experiments::policy::Policy;
use chloedb_experiments::server::Server;
use chloedb_experiments::sim::{Impression, World};
use chloedb_experiments::site::{self, Site};
use chloedb_experiments::users::{Population, Shape};

/// The site's ranking, which logs; and another, which the checks value.
const RANK: &str = "bm25(body, :t, syntax => 'plain') + 0.3 * bm25(title, :t, syntax => 'plain')";
const OTHER: &str = "2 * bm25(title, :t, syntax => 'plain') + bm25(body, :t, syntax => 'plain')";

#[derive(Parser)]
#[command(name = "chloedb-experiments", version, about = "Experiments on ChloeDB: simulated users and click models, logging policies with exact propensities, off-policy estimates checked against the truth")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check every estimator against the truth over many simulated logs.
    Check(CheckArgs),
    /// Run one simulation, served or fast, and check it.
    Sim(SimArgs),
    /// Serve the Experiments app of the command center (chloedb-server --app experiments=URL).
    Serve(ServeArgs),
}

#[derive(Args)]
struct ServeArgs {
    #[command(flatten)]
    target: Conn,
    #[arg(long, env = "CHLOEDB_EXPERIMENTS_LISTEN", default_value = "127.0.0.1:7795")]
    listen: String,
    /// A token every request must carry (Authorization: Bearer); chloedb-server sends its CHLOEDB_APP_TOKEN_EXPERIMENTS. With --local and no token, one is made for the server.
    #[arg(long, env = "CHLOEDB_EXPERIMENTS_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// A directory of BEIR collections (as tests/fixtures/beir/fetch.sh makes) to load as sites.
    #[arg(long)]
    beir: Option<PathBuf>,
    /// The collections of --beir to load.
    #[arg(long, default_value = "nfcorpus,scifact")]
    collections: String,
}

#[derive(Args, Clone)]
struct Conn {
    /// The chloedb-server to rank with (one server, or a coordinator).
    #[arg(long, env = "CHLOEDB_URL")]
    server: Option<String>,
    #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Start a chloedb-server on this machine, its database in this directory.
    #[arg(long)]
    local: Option<PathBuf>,
    /// The chloedb-server binary for --local (default: next to this one, or on the PATH).
    #[arg(long, env = "CHLOEDB_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:7790")]
    local_listen: String,
}

#[derive(Args, Clone)]
struct World0 {
    /// Simulated users.
    #[arg(long, default_value_t = 4000)]
    users: usize,
    /// Rows shown per list.
    #[arg(long, default_value_t = 5)]
    limit: usize,
    #[arg(long, default_value_t = 7)]
    seed: u64,
    /// How much cohorts' tastes move a row's attractiveness (log sd).
    #[arg(long, default_value_t = 0.35)]
    taste: f64,
}

#[derive(Args)]
struct CheckArgs {
    /// A directory of BEIR collections (chloedb/tests/fixtures/beir/fetch.sh DIR).
    #[arg(long, env = "CHLOEDB_BEIR_DIR")]
    beir: PathBuf,
    #[arg(long, default_value = "nfcorpus,scifact")]
    collections: String,
    #[arg(long, default_value = "pbm,cascade,dcm,dbn,ubm,trust")]
    models: String,
    #[arg(long, default_value = "softmax,epsilon,shuffle,swap")]
    policies: String,
    /// Independent logs per case.
    #[arg(long, default_value_t = 100)]
    reps: usize,
    /// Impressions per log.
    #[arg(long, default_value_t = 20_000)]
    impressions: usize,
    /// Where results.md and results.json go.
    #[arg(long, default_value = "results")]
    out: PathBuf,
    /// Use the ranked rows fetched before (fetched-<collection>.json in --out), without a server.
    #[arg(long)]
    offline: bool,
    #[command(flatten)]
    world: World0,
    #[command(flatten)]
    target: Conn,
}

#[derive(Args)]
struct SimArgs {
    /// A BEIR collection's directory.
    #[arg(long)]
    beir: PathBuf,
    #[arg(long, default_value = "fast")]
    mode: String,
    #[arg(long, default_value_t = 100_000)]
    impressions: u64,
    #[arg(long, default_value = "dbn")]
    model: String,
    /// The logging policy, as the rank log's "explore" takes it.
    #[arg(long, default_value = r#"{"policy": "softmax", "tau": 1.0, "top": 10, "by": "rank"}"#)]
    policy: String,
    /// The run's name: its impressions' requests are <run>-<i>.
    #[arg(long, default_value = "run1")]
    run: String,
    /// Served queries in flight.
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
    /// Where the run's summary goes (JSON).
    #[arg(long)]
    out: Option<PathBuf>,
    #[command(flatten)]
    world: World0,
    #[command(flatten)]
    target: Conn,
}

/// The chloedb-server `--local` started, stopped on a failure too.
static LOCAL_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

fn fail(m: impl std::fmt::Display) -> ! {
    eprintln!("chloedb-experiments: {m}");
    let pid = LOCAL_PID.load(Ordering::Relaxed);
    if pid > 0 {
        // SAFETY: a signal to a child this process started.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    std::process::exit(1)
}

fn named_policy(name: &str) -> Policy {
    let j = match name {
        "softmax" => json!({"policy": "softmax", "tau": 1.0, "top": 10, "by": "rank"}),
        "epsilon" => json!({"policy": "epsilon", "epsilon": 0.3, "top": 10}),
        "shuffle" => json!({"policy": "shuffle", "top": 10}),
        "swap" => json!({"policy": "swap", "top": 10}),
        other => serde_json::from_str(other).unwrap_or_else(|e| fail(format!("policy {other:?}: softmax, epsilon, shuffle, swap or JSON ({e})"))),
    };
    Policy::from_json(&j).unwrap_or_else(|e| fail(e))
}

fn server_bin(given: &Option<PathBuf>) -> PathBuf {
    if let Some(b) = given {
        return b.clone();
    }
    let here = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("chloedb-server")));
    let path = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|d| d.join("chloedb-server")).collect::<Vec<_>>();
    here.into_iter().chain(path).find(|p| p.exists()).unwrap_or_else(|| fail("--local needs chloedb-server: give --server-bin, or put it on the PATH"))
}

/// A chloedb-server this process started (`--local`).
struct Local {
    child: Child,
}

impl Local {
    async fn start(dir: &Path, bin: PathBuf, listen: &str, app: Option<(String, Option<String>)>) -> Result<(Local, String), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let url = format!("http://{listen}");
        let s = Server::new(&url, None);
        // Not a server this process did not start (one left by a run that
        // was killed, say), which the check below would take for its own.
        if s.health().await.is_ok() {
            return Err(format!("a server already answers at {url}: stop it, or give --local-listen another address"));
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("chloedb-server.log")).map_err(|e| e.to_string())?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&bin);
        cmd.args(["--db", &dir.join("experiments.strata").to_string_lossy(), "--listen", listen, "--grpc", "off"]);
        // The app this process serves, shown in the server's command center.
        if let Some((url, token)) = &app {
            cmd.args(["--app", &format!("experiments={url}")]);
            if let Some(t) = token {
                cmd.env("CHLOEDB_APP_TOKEN_EXPERIMENTS", t);
            }
        }
        cmd.env("CHLOEDB_START_APPS", "off").stdout(Stdio::from(log)).stderr(Stdio::from(err)).stdin(Stdio::null()).kill_on_drop(true);
        // On Linux the server goes when this process does, however it ends.
        #[cfg(target_os = "linux")]
        // SAFETY: prctl only, between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|e| format!("starting {}: {e}", bin.display()))?;
        let t0 = Instant::now();
        loop {
            if s.health().await.is_ok() {
                LOCAL_PID.store(child.id().unwrap_or(0) as i32, Ordering::Relaxed);
                return Ok((Local { child }, url));
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

    async fn stop(mut self) {
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

async fn connect(t: &Conn) -> (Server, Option<Local>) {
    connect_as(t, None).await
}

async fn connect_as(t: &Conn, app: Option<(String, Option<String>)>) -> (Server, Option<Local>) {
    match (&t.local, &t.server) {
        (Some(_), Some(_)) => fail("give --server or --local, not both"),
        (Some(dir), None) => {
            let (l, url) = Local::start(dir, server_bin(&t.server_bin), &t.local_listen, app).await.unwrap_or_else(|e| fail(e));
            eprintln!("chloedb-experiments: a chloedb-server in {} answers at {url}", dir.display());
            (Server::new(&url, t.key.clone()), Some(l))
        }
        (None, url) => (Server::new(url.as_deref().unwrap_or("http://127.0.0.1:7700"), t.key.clone()), None),
    }
}

fn table_of(dir: &Path) -> String {
    let name: String = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "site".into()).chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    format!("exp_{name}")
}

/// A collection loaded and ranked: its site, ranked rows and the other
/// ranking's lists, fetched (and kept in `out`) or read from there.
async fn fetch(server: Option<&Server>, dir: &Path, w0: &World0, depth: usize, out: &Path) -> (Site, Fetched) {
    let table = table_of(dir);
    let site = Site::beir(dir, &table, "test", w0.seed).unwrap_or_else(|e| fail(e));
    let kept = out.join(format!("fetched-{}.json", site.name));
    let Some(server) = server else {
        let f: Fetched = serde_json::from_slice(&std::fs::read(&kept).unwrap_or_else(|e| fail(format!("{}: {e} (run once with a server)", kept.display())))).unwrap_or_else(|e| fail(e));
        let mut site = site;
        let ids: std::collections::HashSet<&String> = f.queries.iter().collect();
        let keep: Vec<bool> = site.queries.iter().map(|q| ids.contains(&q.id)).collect();
        site.keep(&keep);
        return (site, f);
    };
    let t0 = Instant::now();
    let loaded = site::load_beir(server, dir, &table).await.unwrap_or_else(|e| fail(e));
    if loaded > 0 {
        eprintln!("chloedb-experiments: loaded {loaded} rows of {} into {table} ({:.1} s)", site.name, t0.elapsed().as_secs_f64());
    }
    let mut site = site;
    let mut ranked = World::rank_all(server, &site, RANK, depth).await.unwrap_or_else(|e| fail(e));
    // A query the ranking ranks fewer rows for than a list draws from is
    // not one this site serves.
    let keep: Vec<bool> = ranked.iter().map(|r| r.keys.len() >= depth).collect();
    let dropped = keep.iter().filter(|k| !**k).count();
    if dropped > 0 {
        eprintln!("chloedb-experiments: {dropped} of {} queries rank fewer than {depth} rows: left out", keep.len());
        site.keep(&keep);
        ranked.retain(|r| r.keys.len() >= depth);
    }
    let w = World::new(site.clone(), Population::new(10, w0.seed, Shape::default()), Model::Pbm, Policy::None, w0.limit, RANK.into(), ranked.clone());
    let (rerank, wide) = check::other_ranking(server, &w, OTHER).await.unwrap_or_else(|e| fail(e));
    let f = Fetched { queries: site.queries.iter().map(|q| q.id.clone()).collect(), ranked: ranked.into_iter().map(|r| (r.keys, r.scores)).collect(), rerank, wide };
    std::fs::create_dir_all(out).ok();
    std::fs::write(&kept, serde_json::to_vec(&f).unwrap()).unwrap_or_else(|e| fail(format!("{}: {e}", kept.display())));
    (site, f)
}

fn world(site: &Site, f: &Fetched, w0: &World0, model: Model, policy: Policy) -> World {
    World::new(site.clone(), Population::new(w0.users, w0.seed, Shape { taste: w0.taste, ..Shape::default() }), model, policy, w0.limit, RANK.into(), f.ranked())
}

fn main() {
    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");
    match cli.cmd {
        Cmd::Check(a) => rt.block_on(run_check(a)),
        Cmd::Sim(a) => rt.block_on(run_sim(a)),
        Cmd::Serve(a) => rt.block_on(run_serve(a)),
    }
}

/// 32 random bytes as hex.
fn random_token() -> String {
    let mut b = [0u8; 32];
    match std::fs::File::open("/dev/urandom") {
        Ok(mut f) => {
            use std::io::Read;
            let _ = f.read_exact(&mut b);
        }
        Err(_) => {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
            b[..16].copy_from_slice(&(t ^ ((std::process::id() as u128) << 64)).to_le_bytes());
        }
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

async fn run_serve(a: ServeArgs) {
    let token = a.token.clone().or_else(|| a.target.local.as_ref().map(|_| random_token()));
    let listener = tokio::net::TcpListener::bind(&a.listen).await.unwrap_or_else(|e| fail(format!("{}: {e}", a.listen)));
    let me = format!("http://{}", listener.local_addr().map(|x| x.to_string()).unwrap_or(a.listen.clone()));
    let (server, local) = connect_as(&a.target, Some((me.clone(), token.clone()))).await;
    if local.is_some() {
        eprintln!("chloedb-experiments: open the command center at {} (Apps → Experiments)", server.url);
    } else if std::env::var_os("CHLOEDB_APP_OF").is_none() {
        eprintln!("chloedb-experiments: to see it in the command center, start chloedb-server with --app experiments={me}{}", if token.is_some() { " and CHLOEDB_APP_TOKEN_EXPERIMENTS set to its token" } else { "" });
    }
    let mut beir = Vec::new();
    if let Some(dir) = &a.beir {
        for c in a.collections.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            let d = dir.join(c);
            if !d.join("corpus.jsonl").exists() {
                fail(format!("{}: no BEIR collection there (tests/fixtures/beir/fetch.sh in chloedb fetches them)", d.display()));
            }
            beir.push((d, c.to_ascii_lowercase()));
        }
    }
    let r = chloedb_experiments::app::serve(server, listener, chloedb_experiments::app::Opts { token, beir }).await;
    if let Some(l) = local {
        l.stop().await;
    }
    if let Err(e) = r {
        fail(e);
    }
}

async fn run_check(a: CheckArgs) {
    let models: Vec<Model> = a.models.split(',').map(|m| Model::parse(m.trim()).unwrap_or_else(|e| fail(e))).collect();
    let policies: Vec<Policy> = a.policies.split(',').map(|p| named_policy(p.trim())).collect();
    let depth = policies.iter().map(|p| World::depth(p, a.world.limit)).max().unwrap_or(10).max(10);
    let (server, local) = if a.offline { (None, None) } else { let (s, l) = connect(&a.target).await; (Some(s), l) };
    let mut cases = Vec::new();
    let mut swaps = Vec::new();
    let t0 = Instant::now();
    for c in a.collections.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        let (site, f) = fetch(server.as_ref(), &a.beir.join(c), &a.world, depth, &a.out).await;
        eprintln!("chloedb-experiments: {} has {} judged queries", site.name, site.queries.len());
        for &model in &models {
            for policy in &policies {
                let w = world(&site, &f, &a.world, model, policy.clone());
                let targets = check::targets(&w, &f.rerank, &f.wide);
                let label = format!("{}-{}-{}", site.name, model.name(), policy.to_json()["policy"].as_str().unwrap_or("p"));
                let w = Arc::new(w);
                let (w2, targets2, reps, n) = (w.clone(), targets.clone(), a.reps, a.impressions);
                let r = tokio::task::spawn_blocking(move || check::run_case(&w2, &targets2, reps, n, &label)).await.expect("a case");
                let fails = r.failures();
                eprintln!("chloedb-experiments: {} {} {}: {:.0} s, {}", site.name, model.name(), r.policy, r.seconds, if fails.is_empty() { "ok".to_string() } else { format!("{} failed", fails.len()) });
                cases.push(r);
            }
        }
        // Position bias from a swap log, under PBM.
        let w = Arc::new(world(&site, &f, &a.world, Model::Pbm, named_policy("swap")));
        let n = a.reps * a.impressions;
        let label = format!("{}-swap", site.name);
        let s = tokio::task::spawn_blocking(move || check::swap_check(&w, n, &label)).await.expect("a swap check");
        swaps.push((site.name.clone(), s));
    }
    if let Some(l) = local {
        l.stop().await;
    }
    let note = format!(
        "Written by `chloedb-experiments check` ({} users, {} rows shown, seed {}, taste {}; logging ranking `{RANK}`, `rerank` and `wide` by `{OTHER}`) in {:.0} s on {} ({} threads).",
        a.world.users,
        a.world.limit,
        a.world.seed,
        a.world.taste,
        t0.elapsed().as_secs_f64(),
        machine(),
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
    );
    let md = check::markdown(&cases, &swaps, &note);
    std::fs::create_dir_all(&a.out).ok();
    std::fs::write(a.out.join("results.md"), &md).unwrap_or_else(|e| fail(e));
    std::fs::write(a.out.join("results.json"), serde_json::to_vec_pretty(&json!({"note": note, "cases": cases, "swaps": swaps})).unwrap()).unwrap_or_else(|e| fail(e));
    let fails: Vec<String> = cases.iter().flat_map(|c| c.failures()).chain(swaps.iter().flat_map(|(_, s)| s.failures())).collect();
    println!("{}", a.out.join("results.md").display());
    if !fails.is_empty() {
        for f in &fails {
            eprintln!("FAIL {f}");
        }
        std::process::exit(1);
    }
}

/// The CPU and memory of this machine, for results' notes.
fn machine() -> String {
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().to_string())).unwrap_or_else(|| std::env::consts::ARCH.into());
    let mem = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| s.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|k| k.parse::<f64>().ok())).map(|kb| format!(", {:.1} GB", kb / 1e6)).unwrap_or_default();
    format!("{cpu}{mem}")
}

/// Splits `n` impressions over the machine's threads.
fn par_chunks<T: Send>(n: u64, f: impl Fn(std::ops::Range<u64>) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4) as u64;
    let per = n.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads).map(|t| (t * per, ((t + 1) * per).min(n))).filter(|(a, b)| a < b).map(|(a, b)| { let f = &f; s.spawn(move || f(a..b)) }).collect();
        hs.into_iter().map(|h| h.join().expect("a thread")).collect()
    })
}

fn summary(w: &World, targets: &[Target], v: &[Valuer]) -> Vec<Value> {
    targets
        .iter()
        .zip(v)
        .map(|(t, v)| {
            let truth = w.truth(&t.keys, &|_| true);
            let rows: Vec<Value> = v
                .estimates()
                .iter()
                .map(|e| {
                    let tr = truth.metric(e.reward);
                    json!({"estimator": e.estimator, "reward": e.reward, "value": e.value, "se": e.se, "truth": tr, "z": (e.value - tr) / e.se, "covers": e.covers(tr), "holds": estimate::should_hold(e.estimator, w.model) && t.unsupported(w) == 0.0, "ess": e.ess, "weight_mean": e.weight_mean, "weight_z": e.weight_z, "shown": e.trusted()})
                })
                .collect();
            json!({"target": t.name, "unsupported": t.unsupported(w), "truth": truth, "estimates": rows})
        })
        .collect()
}

async fn run_sim(a: SimArgs) {
    let model = Model::parse(&a.model).unwrap_or_else(|e| fail(e));
    let policy = named_policy(&a.policy);
    if !a.run.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        fail("--run is letters, digits and _");
    }
    let depth = World::depth(&policy, a.world.limit);
    let out_dir = a.out.as_ref().and_then(|o| o.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."));
    let (server, local) = connect(&a.target).await;
    let (site, f) = fetch(Some(&server), &a.beir, &a.world, depth, &out_dir).await;
    let w = Arc::new(world(&site, &f, &a.world, model, policy));
    let targets = check::targets(&w, &f.rerank, &f.wide);
    let n = a.impressions;
    let mut report = json!({"site": site.name, "model": model.name(), "policy": w.policy.to_json(), "mode": a.mode, "impressions": n, "users": a.world.users, "machine": machine()});
    match a.mode.as_str() {
        "fast" => {
            // Pass one: the models' counts and the propensities' tallies;
            // pass two, the same impressions again: every estimator.
            let t0 = Instant::now();
            let parts = par_chunks(n, |r| {
                let mut c = [Counts::default(), Counts::default()];
                let mut clicks = 0u64;
                for i in r {
                    let imp = w.fast(&a.run, i);
                    clicks += imp.click.iter().filter(|&&c| c).count() as u64;
                    c[estimate::fold(i)].add(&imp);
                }
                (c, clicks)
            });
            let first = t0.elapsed().as_secs_f64();
            let mut counts = [Counts::default(), Counts::default()];
            let mut clicks = 0;
            for (c, k) in parts {
                clicks += k;
                for (f, part) in c.into_iter().enumerate() {
                    for (key, v) in part.cells {
                        let e = counts[f].cells.entry(key).or_default();
                        (0..3).for_each(|x| e[x] += v[x]);
                    }
                    for (key, v) in part.rows {
                        let e = counts[f].rows.entry(key).or_default();
                        (0..4).for_each(|x| e[x] += v[x]);
                    }
                    counts[f].depth = counts[f].depth.max(part.depth);
                }
            }
            let models = Models::fit(&counts);
            let t1 = Instant::now();
            let valuers: Vec<Vec<Valuer>> = par_chunks(n, |r| {
                let mut vs: Vec<Valuer> = targets.iter().map(|t| Valuer::new(t, Some(&models), w.limit)).collect();
                for i in r {
                    let imp = w.fast(&a.run, i);
                    vs.iter_mut().for_each(|v| v.add(&imp));
                }
                vs
            });
            let mut merged: Vec<Valuer> = targets.iter().map(|t| Valuer::new(t, Some(&models), w.limit)).collect();
            for vs in valuers {
                for (m, v) in merged.iter_mut().zip(vs) {
                    m.merge(v);
                }
            }
            let second = t1.elapsed().as_secs_f64();
            report["seconds"] = json!({"generate_and_count": first, "generate_and_estimate": second});
            report["impressions_per_s"] = json!(n as f64 / first);
            report["clicks"] = json!(clicks);
            report["targets"] = json!(summary(&w, &targets, &merged));
        }
        "served" => {
            let log = format!("exp_log_{}", a.run);
            let events = format!("exp_events_{}", a.run);
            server.sql(&format!("CREATE TABLE IF NOT EXISTS {events} (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD, action KEYWORD, ts INT, dwell_ms FLOAT)"), json!({})).await.unwrap_or_else(|e| fail(e));
            let next = Arc::new(AtomicU64::new(0));
            let done = Arc::new(AtomicU64::new(0));
            let t0 = Instant::now();
            let mut tasks = Vec::new();
            for _ in 0..a.concurrency.max(1) {
                let (w, server, next, done, run, log, events) = (w.clone(), server.clone(), next.clone(), done.clone(), a.run.clone(), log.clone(), events.clone());
                tasks.push(tokio::spawn(async move {
                    let mut got: Vec<Impression> = Vec::new();
                    let mut ops: Vec<Value> = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            break;
                        }
                        let (imp, o) = w.served(&server, &run, &log, &events, i).await?;
                        ops.extend(o);
                        if ops.len() >= 500 {
                            server.write(std::mem::take(&mut ops)).await?;
                        }
                        got.push(imp);
                        let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if d % 50_000 == 0 {
                            eprintln!("chloedb-experiments: {d} of {n} served");
                        }
                    }
                    if !ops.is_empty() {
                        server.write(ops).await?;
                    }
                    Ok::<_, String>(got)
                }));
            }
            let mut served: Vec<Impression> = Vec::new();
            for t in tasks {
                served.extend(t.await.expect("a task").unwrap_or_else(|e| fail(e)));
            }
            let secs = t0.elapsed().as_secs_f64();
            served.sort_by_key(|x| x.i);
            // Served is fast: each impression as drawn here.
            let differ = served.iter().filter(|s| **s != Impression { took_ms: s.took_ms, ..w.fast(&a.run, s.i) }).count();
            // The log, read back through the export, is what was served.
            let t1 = Instant::now();
            let back = w.read_back(&server, &a.run, &log, &events).await.unwrap_or_else(|e| fail(e));
            let read_s = t1.elapsed().as_secs_f64();
            let lost = served.len() - back.len().min(served.len());
            let back_differ = back.iter().zip(&served).filter(|(b, s)| b.i != s.i || b.shown != s.shown || b.propensity != s.propensity || b.prefix != s.prefix || b.click != s.click || b.satisfied != s.satisfied).count();
            let mut took: Vec<f64> = served.iter().map(|s| s.took_ms).collect();
            took.sort_by(f64::total_cmp);
            let pct = |p: f64| took.get(((took.len() as f64 - 1.0) * p) as usize).copied().unwrap_or(0.0);
            let models = {
                let mut c = [Counts::default(), Counts::default()];
                back.iter().for_each(|imp| c[estimate::fold(imp.i)].add(imp));
                Models::fit(&c)
            };
            let vs: Vec<Valuer> = targets
                .iter()
                .map(|t| {
                    let mut v = Valuer::new(t, Some(&models), w.limit);
                    back.iter().for_each(|imp| v.add(imp));
                    v
                })
                .collect();
            report["seconds"] = json!({"serve": secs, "read_back": read_s});
            report["impressions_per_s"] = json!(n as f64 / secs);
            report["concurrency"] = json!(a.concurrency);
            report["query_ms"] = json!({"p50": pct(0.5), "p90": pct(0.9), "p99": pct(0.99)});
            report["served_differs_from_fast"] = json!(differ);
            report["log_lost"] = json!(lost);
            report["log_differs_from_served"] = json!(back_differ);
            report["targets"] = json!(summary(&w, &targets, &vs));
            if differ > 0 || lost > 0 || back_differ > 0 {
                eprintln!("chloedb-experiments: FAIL: {differ} served impressions differ from fast, {lost} lost from the log, {back_differ} differ in it");
            }
        }
        m => fail(format!("--mode is fast or served, not {m}")),
    }
    if let Some(l) = local {
        l.stop().await;
    }
    let text = serde_json::to_string_pretty(&report).unwrap();
    match &a.out {
        Some(p) => std::fs::write(p, &text).unwrap_or_else(|e| fail(e)),
        None => println!("{text}"),
    }
}
