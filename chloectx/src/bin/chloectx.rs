//! chloectx: automatic long-term context for Claude Code, stored in ChloeDB.
//! (`engram`, its name before the rename, is a link to it for a release.)
//!
//!   chloectx install [--project DIR] [--shared]   wire hooks + MCP into a project
//!   chloectx install --user                        ... or into every Claude Code session
//!   chloectx uninstall [--user]                    remove them again (stores are kept)
//!   chloectx hook [--user] <Event>                 (called by Claude Code hooks)
//!   chloectx mcp                                   (MCP stdio server, started by Claude Code)
//!   chloectx search "query" | sql "SELECT ..." | stats | briefing | stop [--all]
//!   chloectx remember <kind> "text"
//!   chloectx ingest-transcript <file.jsonl>        back-fill from a Claude Code transcript
//!   chloectx redact [--all] [--dry-run]            redact secrets a store recorded before
//!   chloectx fetch-model [static|bert]             download the embedding model
//!   chloectx sync setup --url URL [--token-stdin]  copy the memory to a ChloeDB server (status|push|pull)
//!   chloectx doctor                                check the install and the project's store
//!   chloectx ui [--port N] [--home DIR]            browse every local store in the inspector
//!   chloectx pack OUT [--kinds ..] [--since-days N] cut a capsule another agent can mount
//!   chloectx mount DIR...                          serve capsules as MCP tools (stdio)
//!   chloectx rpc '{"op": ...}'                     send one raw request to the daemon
//!   chloectx serve --dir D --project P             (the daemon; started on demand)

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serde_json::{json, Value as Json};

use chloectx::{daemon, doctor, embed, hook, install, mcp, paths, transcript, ui};

#[derive(Parser)]
#[command(name = "chloectx", version, about = "Automatic long-term context for Claude Code, stored in ChloeDB")]
struct Cli {
    /// Project directory (default: $CLAUDE_PROJECT_DIR or the current directory).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Install hooks and the MCP server into the project's Claude Code settings.
    Install {
        /// Write hooks to .claude/settings.json (committed) instead of settings.local.json.
        #[arg(long)]
        shared: bool,
        /// Install for every Claude Code session: hooks in ~/.claude/settings.json,
        /// the MCP server at user scope. Each project still gets its own store.
        #[arg(long, conflicts_with = "shared")]
        user: bool,
        /// Path of the chloectx binary to call (default: this one).
        #[arg(long)]
        exe: Option<String>,
        /// Model to distill turns with (sets CHLOECTX_DISTILL_MODEL in the settings' env).
        #[arg(long)]
        distill_model: Option<String>,
    },
    /// Remove what `install` wrote (the project's, or with --user the user-level one); stores are kept.
    Uninstall {
        #[arg(long)]
        user: bool,
    },
    /// Handle a Claude Code hook event (payload on stdin).
    Hook {
        /// Called from the user settings: step aside in a project with its own install.
        #[arg(long)]
        user: bool,
        event: String,
    },
    /// Run the MCP server on stdio.
    Mcp,
    /// Run the daemon in the foreground.
    Serve {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Search the project's memory.
    Search {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long, value_delimiter = ',')]
        kinds: Vec<String>,
    },
    /// Answer a question about earlier work: the rules, notes, replies and
    /// messages that bear on it, found by search alone and packed for it.
    Ask {
        question: String,
        /// A model plans several searches and a second call picks the records.
        #[arg(long)]
        deep: bool,
        /// With --deep: model for planning and ranking (default: the configured one).
        #[arg(long)]
        model: Option<String>,
        /// Print the JSON result (picked ids, answer, confidence, cost).
        #[arg(long)]
        json: bool,
    },
    /// Run read-only SQL over the memory table.
    Sql { sql: String },
    /// Store a decision, preference, fact, todo or note.
    Remember { kind: String, text: String },
    /// Counts and sizes.
    Stats,
    /// Copy the project's store to a new directory while ChloeCtx runs: a
    /// consistent copy of the database, the session state and the event
    /// log. To restore, run `chloectx stop` and copy the files back.
    Backup { dest: PathBuf },
    /// Print the session-start briefing.
    Briefing,
    /// Write down the decisions, rules, requirements and facts of every
    /// stored turn (for a store recorded before distillation existed).
    Distill,
    /// Merge related notes now (it also runs on its own every few turns).
    Consolidate,
    /// Redact the secrets a store recorded before redaction knew their shape:
    /// rewrites the project's records (with --all, every store's), session
    /// state and event log, compacts the store, and replaces the records on
    /// the sync's server. Prints how many changed, never the secrets.
    Redact {
        #[arg(long)]
        all: bool,
        /// Count what would change, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop the daemon (with --all, every running daemon, e.g. after an upgrade).
    Stop {
        #[arg(long)]
        all: bool,
    },
    /// Check that ChloeCtx is wired into Claude Code and the project's daemon answers.
    Doctor,
    /// Serve the inspector over every ChloeCtx store on this machine.
    Ui {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port (the next free one up to 9 above it is used if it is taken).
        #[arg(long, default_value_t = 8765)]
        port: u16,
        /// Directory holding projects/<store>/ (default: $CHLOECTX_HOME, or ~/.local/share/chloectx;
        /// ~/.local/share/engram on a machine that has only that one).
        #[arg(long)]
        home: Option<PathBuf>,
        /// Do not open a browser.
        #[arg(long)]
        no_open: bool,
    },
    /// Serve ChloeCtx as an app of a ChloeDB cluster (`chloedb-server --app
    /// chloectx=URL`): memory spaces any agent writes to and asks for
    /// context, kept in the cluster's tables, and Seen before, the cluster's
    /// firing alerts beside the past incidents they look like.
    App {
        /// The chloedb-server (a coordinator) the spaces live in.
        #[arg(long, env = "CHLOEDB_URL", default_value = "http://127.0.0.1:7700")]
        server: String,
        /// Its key, for the app's reads and writes ($CHLOEDB_KEY).
        #[arg(long, env = "CHLOEDB_KEY", hide_env_values = true)]
        key: Option<String>,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7760)]
        port: u16,
        /// The token callers must send as `Authorization: Bearer` (chloedb-server
        /// sends its CHLOEDB_APP_TOKEN); none: any caller ($CHLOECTX_APP_TOKEN).
        #[arg(long, env = "CHLOECTX_APP_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Serve any caller with no token on an address other than loopback
        /// (without it, the app refuses to start that way).
        #[arg(long)]
        insecure: bool,
        /// The embedder (static, or hash:<dim> for tests; $CHLOECTX_EMBEDDER).
        #[arg(long, env = "CHLOECTX_EMBEDDER", default_value = "static")]
        embedder: String,
    },
    /// Cut a capsule from the project's memory: a read-only ChloeDB file of
    /// the selected records (default: distilled notes, entities and the
    /// conversation, no raw tool output), redacted again, with a manifest.
    /// Another agent mounts it with `chloectx mount`.
    Pack {
        /// The capsule's directory (must not exist).
        out: PathBuf,
        /// Record kinds to include (default: decision, preference, fact, todo,
        /// note, entity, prompt, answer, episode, summary, error; `all` for every kind).
        #[arg(long, value_delimiter = ',')]
        kinds: Vec<String>,
        /// Only records from the last N days.
        #[arg(long)]
        since_days: Option<f64>,
        /// At most this many records (the oldest first).
        #[arg(long, default_value_t = 200_000)]
        max_records: usize,
        /// The capsule's title (default: the project's folder name).
        #[arg(long)]
        title: Option<String>,
        /// Print the manifest as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Serve capsules to Claude Code as an MCP server on stdio
    /// (capsule_ask, capsule_search, capsule_timeline, capsule_sql).
    Mount {
        /// Capsule directories.
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        /// Append one JSON line per tool call (and the open) to this file ($CHLOECTX_CAPSULE_LOG).
        #[arg(long)]
        log: Option<PathBuf>,
        /// Embedder spec, instead of the one each manifest names.
        #[arg(long)]
        embedder: Option<String>,
    },
    /// Send one raw JSON request to the daemon and print its JSON response
    /// (the demo inspector's transport).
    Rpc {
        request: String,
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    /// Back-fill records from a Claude Code transcript.
    IngestTranscript { path: PathBuf },
    /// Copy the project's memory to a ChloeDB server or cluster, and back.
    Sync {
        #[command(subcommand)]
        what: SyncCmd,
    },
    /// Download an embedding model (static: potion-retrieval-32M; bert: bge-small-en-v1.5).
    FetchModel {
        #[arg(default_value = "static")]
        which: String,
    },
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Name the server (e.g. https://chloedb.example.com) and create its table.
    Setup {
        #[arg(long)]
        url: String,
        /// Read the server's key (an API key or token) from stdin, kept
        /// owner-only in ChloeCtx's home, never on the command line.
        #[arg(long, conflicts_with = "token_env")]
        token_stdin: bool,
        /// Read the key from this environment variable whenever the daemon
        /// syncs (one a cloud environment sets, say); no file holds it.
        #[arg(long, default_value = "")]
        token_env: String,
        /// A repository renamed, as owner/old=owner/new (repeatable): a
        /// checkout under either name syncs as the new one.
        #[arg(long = "rename")]
        renames: Vec<String>,
        /// Do not create the table (it exists, or the key cannot: it needs the admin scope).
        #[arg(long)]
        no_create: bool,
    },
    /// What was sent and received, and what the server holds for this project.
    Status,
    /// Send what the store recorded since the last push.
    Push,
    /// Push, then store the project's records the server holds and the store lacks.
    Pull,
}

fn send(project: &std::path::Path, req: Json, secs: u64) -> Json {
    let dir = paths::store_dir(project);
    match daemon::request(&dir, &project.to_string_lossy(), &req, Duration::from_secs(secs)) {
        Ok(r) => {
            if let Some(e) = r.get("error") {
                eprintln!("chloectx: {}", e.as_str().unwrap_or("error"));
                std::process::exit(1);
            }
            r
        }
        Err(e) => {
            eprintln!("chloectx: {e}");
            std::process::exit(1);
        }
    }
}

/// `chloectx redact` for one store: the daemon rewrites the records, then
/// (when it changed any) the file is replaced by a copy without the pages
/// their old versions were on. What it did, without the secrets.
fn redact_store(dir: &std::path::Path, project: &str, dry_run: bool) -> Result<String, String> {
    let r = daemon::request(dir, project, &json!({"op": "redact", "dry_run": dry_run}), Duration::from_secs(6 * 3600)).map_err(|e| e.to_string())?;
    if let Some(e) = r.get("error") {
        return Err(e.as_str().unwrap_or("error").to_string());
    }
    let mut out = r["text"].as_str().unwrap_or("").to_string();
    if !dry_run && r["changed"].as_u64().unwrap_or(0) > 0 {
        match daemon::rewrite_file(dir, Duration::from_secs(600)) {
            Ok(f) => out += &format!(" The store's file was rewritten without the old versions ({} to {} bytes).", f["bytes_before"], f["bytes_after"]),
            Err(e) => return Err(format!("{out} Rewriting the store's file failed, so the old versions may stay in its free pages until they are reused: {e}")),
        }
    }
    Ok(out)
}

fn text(r: &Json) {
    println!("{}", r.get("text").and_then(|t| t.as_str()).unwrap_or(""));
}

fn main() {
    chloectx::env::adopt_old_names();
    let cli = Cli::parse();
    let project = cli.project.clone().unwrap_or_else(|| paths::project_dir(None));
    let project = project.canonicalize().unwrap_or(project);
    match cli.cmd {
        Cmd::Hook { event, user } => std::process::exit(hook::run(&event, user)),
        Cmd::Mcp => std::process::exit(mcp::run()),
        Cmd::Serve { dir } => {
            let dir = dir.unwrap_or_else(|| paths::store_dir(&project));
            if let Err(e) = daemon::serve(&dir, &project.to_string_lossy()) {
                eprintln!("chloectx serve: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Install { shared, user, exe, distill_model } => {
            let exe = exe.unwrap_or_else(|| std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "chloectx".into()));
            let dm = distill_model.as_deref();
            match if user { install::install_user(&exe, dm) } else { install::install(&project, &exe, shared, dm) } {
                Ok(s) => println!("{s}"),
                Err(e) => {
                    eprintln!("chloectx install: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Uninstall { user } => match if user { install::uninstall_user() } else { install::uninstall(&project) } {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("chloectx uninstall: {e}");
                std::process::exit(1);
            }
        },
        Cmd::Search { query, limit, kinds } => text(&send(&project, json!({"op": "search", "query": query, "limit": limit, "kinds": kinds, "include_current": true}), 30)),
        Cmd::Ask { question, deep, model, json: as_json } => {
            let r = send(&project, json!({"op": "ask", "question": question, "deep": deep || model.is_some(), "model": model, "include_current": true}), 300);
            if as_json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap());
            } else {
                text(&r);
            }
        }
        Cmd::Sql { sql } => text(&send(&project, json!({"op": "sql", "sql": sql}), 30)),
        Cmd::Remember { kind, text: t } => text(&send(&project, json!({"op": "remember", "kind": kind, "text": t}), 30)),
        Cmd::Stats => println!("{}", serde_json::to_string_pretty(&send(&project, json!({"op": "stats"}), 30)).unwrap()),
        Cmd::Backup { dest } => {
            let dest = std::path::absolute(&dest).unwrap_or(dest);
            let r = send(&project, json!({"op": "backup", "dest": dest}), 3600);
            println!("{}: {} records, {} bytes, in {:.1} s", dest.display(), r["records"], r["bytes"], r["seconds"].as_f64().unwrap_or(0.0));
        }
        Cmd::Briefing => text(&send(&project, json!({"op": "briefing", "session": ""}), 30)),
        Cmd::Distill => println!("{}", send(&project, json!({"op": "distill_backfill"}), 6 * 3600)),
        Cmd::Consolidate => println!("{}", send(&project, json!({"op": "consolidate"}), 3600)["report"]),
        Cmd::Rpc { request, timeout } => {
            let req: Json = serde_json::from_str(&request).unwrap_or_else(|e| {
                eprintln!("chloectx: request is not JSON: {e}");
                std::process::exit(2)
            });
            let dir = paths::store_dir(&project);
            match daemon::request(&dir, &project.to_string_lossy(), &req, Duration::from_secs(timeout)) {
                Ok(r) => println!("{r}"),
                Err(e) => {
                    println!("{}", json!({"error": e.to_string()}));
                    std::process::exit(1);
                }
            }
        }
        Cmd::Sync { what: SyncCmd::Setup { url, token_stdin, token_env, renames, no_create } } => {
            let token = token_stdin.then(|| {
                let mut t = String::new();
                let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut t);
                t
            });
            let home = paths::home();
            if let Err(e) = chloectx::sync::setup(&home, &url, token.as_deref(), &token_env, &renames) {
                eprintln!("chloectx sync: {e}");
                std::process::exit(1);
            }
            let remote = chloectx::sync::remote(&home).expect("just set up");
            if !no_create {
                if let Err(e) = remote.create_table() {
                    eprintln!("chloectx sync: could not create the table {} on {}: {e}", chloectx::sync::TABLE, remote.url);
                    std::process::exit(1);
                }
            }
            // Not fatal: the key may only be in the environment of the
            // sessions to come (a cloud setup script runs before it is).
            let key = chloectx::sync::project_key(&home, &project.to_string_lossy());
            match remote.count(&key) {
                Ok(n) => println!("sync set up with {}; this project syncs as {key} ({n} records there)", remote.url),
                Err(e) => println!("sync set up with {}; this project syncs as {key}; the server does not answer yet ({e}): the daemon tries again when it starts", remote.url),
            }
            // A running daemon syncs once it starts again (on the next hook or tool).
            let dir = paths::store_dir(&project);
            if std::os::unix::net::UnixStream::connect(paths::StorePaths::new(&dir).socket).is_ok() {
                send(&project, json!({"op": "shutdown"}), 10);
            }
        }
        Cmd::Sync { what } => {
            let w = match what {
                SyncCmd::Status => "status",
                SyncCmd::Push => "push",
                SyncCmd::Pull => "pull",
                SyncCmd::Setup { .. } => unreachable!(),
            };
            println!("{}", serde_json::to_string_pretty(&send(&project, json!({"op": "sync", "what": w}), 600)).unwrap());
        }
        Cmd::Doctor => std::process::exit(doctor::run(&project)),
        Cmd::Pack { out, kinds, since_days, max_records, title, json: as_json } => {
            let t0 = std::time::Instant::now();
            let out = std::path::absolute(&out).unwrap_or(out);
            let kinds: Vec<chloectx::record::Kind> = if kinds.iter().any(|k| k == "all") {
                chloectx::record::Kind::ALL.to_vec()
            } else if kinds.is_empty() {
                chloectx::capsule::DEFAULT_KINDS.to_vec()
            } else {
                kinds.iter().map(|k| chloectx::record::Kind::parse(k).unwrap_or_else(|| {
                    eprintln!("chloectx pack: unknown kind {k}");
                    std::process::exit(2)
                })).collect()
            };
            // A consistent copy of the store, which no daemon writes while it is cut.
            let tmp = std::env::temp_dir().join(format!("chloectx-pack-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            let _ = std::fs::create_dir_all(&tmp);
            let backup = tmp.join("backup");
            send(&project, json!({"op": "backup", "dest": backup}), 3600);
            let backup_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let spec = daemon::Config::load(&backup.join("config.json")).embedder;
            let fail = |e: chloectx::Error| -> ! {
                let _ = std::fs::remove_dir_all(&tmp);
                eprintln!("chloectx pack: {e}");
                std::process::exit(1)
            };
            let embedder: std::sync::Arc<dyn embed::Embedder> = std::sync::Arc::from(embed::open(&spec).unwrap_or_else(|e| fail(e)));
            let store = chloectx::store::Store::open(&backup.join("memory.strata"), embedder, chloectx::store::StoreOptions { sync: false, ..Default::default() }).unwrap_or_else(|e| fail(e));
            let opts = chloectx::capsule::PackOptions { kinds, since_ts: since_days.map(|d| chloectx::now() - (d * 86_400.0) as i64), max_records, title };
            let m = chloectx::capsule::pack(&store, &spec, &project.to_string_lossy(), &out, &opts).unwrap_or_else(|e| fail(e));
            drop(store);
            let _ = std::fs::remove_dir_all(&tmp);
            let total_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if as_json {
                let mut j = serde_json::to_value(&m).unwrap();
                j["backup_ms"] = json!(backup_ms);
                j["total_ms"] = json!(total_ms);
                j["dir"] = json!(out);
                println!("{}", serde_json::to_string_pretty(&j).unwrap());
            } else {
                println!("{}: {}", out.display(), chloectx::capsule::describe(&m));
                println!("{} bytes, sha256 {}, {} redacted, packed in {:.0} ms", m.bytes, m.sha256, m.redacted, total_ms);
            }
        }
        Cmd::Mount { dirs, log, embedder } => std::process::exit(chloectx::capsule::run_mcp(&dirs, log, embedder.as_deref())),
        Cmd::App { server, key, host, port, token, insecure, embedder } => {
            if let Err(e) = chloectx::app::run(chloectx::app::Options { host, port, server, key, token: token.filter(|t| !t.is_empty()), insecure, embedder }) {
                eprintln!("chloectx app: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Ui { host, port, home, no_open } => {
            let home = home.map(|h| h.canonicalize().unwrap_or(h)).unwrap_or_else(paths::home);
            if let Err(e) = ui::run(ui::Options { host, port, home, open: !no_open, project: project.clone() }) {
                eprintln!("chloectx ui: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Redact { all, dry_run } => {
            let stores: Vec<(PathBuf, String, String)> = if all {
                ui::list_stores(&paths::home()).into_iter().map(|s| (s.dir, s.project.clone().unwrap_or_default(), s.project.unwrap_or(s.id))).collect()
            } else {
                let p = project.to_string_lossy().into_owned();
                vec![(paths::store_dir(&project), p.clone(), p)]
            };
            let mut failed = 0;
            for (dir, proj, name) in stores {
                let prefix = if all { format!("{name}: ") } else { String::new() };
                match redact_store(&dir, &proj, dry_run) {
                    Ok(t) => println!("{prefix}{t}"),
                    Err(e) => {
                        failed += 1;
                        eprintln!("chloectx redact: {prefix}{e}");
                    }
                }
            }
            if failed > 0 {
                std::process::exit(1);
            }
        }
        Cmd::Stop { all: false } => {
            send(&project, json!({"op": "shutdown"}), 10);
        }
        Cmd::Stop { all: true } => {
            let mut n = 0;
            for s in ui::list_stores(&paths::home()) {
                let sock = paths::StorePaths::new(&s.dir).socket;
                if std::os::unix::net::UnixStream::connect(&sock).is_ok()
                    && daemon::request(&s.dir, s.project.as_deref().unwrap_or(""), &json!({"op": "shutdown"}), Duration::from_secs(10)).is_ok()
                {
                    n += 1;
                }
            }
            println!("stopped {n} daemon{}; each starts again when a hook or tool needs it", if n == 1 { "" } else { "s" });
        }
        Cmd::IngestTranscript { path } => {
            let events = transcript::read(&path).unwrap_or_else(|e| {
                eprintln!("chloectx: {e}");
                std::process::exit(1)
            });
            let batches = transcript::drafts(&events, &project.to_string_lossy());
            let mut n = 0;
            for b in &batches {
                let r = send(&project, json!({"op": "append", "session": b.session, "turn": b.turn, "ts": b.ts, "drafts": b.drafts}), 120);
                n += r.get("stored").and_then(|v| v.as_u64()).unwrap_or(0);
            }
            println!("ingested {} events as {n} records from {}", events.len(), path.display());
        }
        Cmd::FetchModel { which } => {
            let &(_, repo, name, files) = embed::FETCHED.iter().find(|m| m.0 == which).unwrap_or(&embed::FETCHED[0]);
            let dir = embed::models_dir().join(name);
            match embed::fetch_model("https://huggingface.co", repo, &dir, files) {
                Ok(done) if done.iter().all(|f| matches!(f, embed::Fetched::Kept(_))) => println!("{repo} is already in {}, as pinned", dir.display()),
                Ok(_) => println!("downloaded {repo} to {}, each file checked against its pinned SHA-256", dir.display()),
                Err(e) => {
                    eprintln!("chloectx: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
