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
//!   chloectx fetch-model [static|bert]             download the embedding model
//!   chloectx sync setup --url URL [--token-stdin]  copy the memory to a ChloeDB server (status|push|pull)
//!   chloectx doctor                                check the install and the project's store
//!   chloectx ui [--port N] [--home DIR]            browse every local store in the inspector
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
        Cmd::Ui { host, port, home, no_open } => {
            let home = home.map(|h| h.canonicalize().unwrap_or(h)).unwrap_or_else(paths::home);
            if let Err(e) = ui::run(ui::Options { host, port, home, open: !no_open, project: project.clone() }) {
                eprintln!("chloectx ui: {e}");
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
            let (repo, name, files): (&str, &str, &[&str]) = match which.as_str() {
                "bert" => ("BAAI/bge-small-en-v1.5", "bge-small-en-v1.5", &["config.json", "tokenizer.json", "model.safetensors"]),
                _ => ("minishlab/potion-retrieval-32M", "potion-retrieval-32M", &["config.json", "tokenizer.json", "model.safetensors"]),
            };
            let dir = embed::models_dir().join(name);
            std::fs::create_dir_all(&dir).expect("create model dir");
            for f in files {
                let url = format!("https://huggingface.co/{repo}/resolve/main/{f}");
                let ok = std::process::Command::new("curl").args(["-fsSL", "-o"]).arg(dir.join(f)).arg(&url).status().map_or(false, |s| s.success());
                if !ok {
                    eprintln!("chloectx: failed to download {url}");
                    std::process::exit(1);
                }
            }
            println!("downloaded {repo} to {}", dir.display());
        }
    }
}
