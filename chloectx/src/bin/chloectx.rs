//! chloectx: automatic long-term context for Claude Code, stored in ChloeDB.
//! (`engram`, its name before the rename, is a link to it for a release.)
//!
//! `chloectx --help` lists the everyday commands; the others are hidden
//! from it and named in [`MORE`], which a test keeps complete.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serde_json::{json, Value as Json};

use chloectx::{daemon, doctor, embed, hook, install, mcp, paths, transcript, ui};

/// The commands `--help` does not list, by what they are for.
const MORE: &str = "\
Once installed, ChloeCtx works on its own: start Claude Code in a project and work
as usual; it records each session, recalls what bears on each prompt and writes
down decisions and rules. `chloectx doctor` says whether that is happening.

More commands (`chloectx help <command>` explains each):
  look into memory   report, repeats, blame, receipts, receipt, snapshot, sql, stats, briefing
  maintain a store   forget, move, redact, distill, consolidate, backup, fetch-model, ingest-transcript
  share memory       sync, pack, mount, app
  run by Claude Code (not by hand): hook, mcp, serve, rpc, wiring";

#[derive(Parser)]
#[command(name = "chloectx", version = env!("CHLOECTX_VERSION_LINE"), about = "Automatic long-term context for Claude Code, stored in ChloeDB", after_help = MORE)]
struct Cli {
    /// Project directory (default: $CLAUDE_PROJECT_DIR or the current directory).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Wire ChloeCtx into Claude Code (a project's, or with --user every session)
    #[command(display_order = 1)]
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
    /// Remove what `install` wrote (stores are kept)
    ///
    /// The project's install, or with --user the user-level one.
    #[command(display_order = 2)]
    Uninstall {
        #[arg(long)]
        user: bool,
    },
    /// Handle a Claude Code hook event (payload on stdin).
    #[command(hide = true)]
    Hook {
        /// Called from the user settings: step aside in a project with its own install.
        #[arg(long)]
        user: bool,
        event: String,
    },
    /// Run the MCP server on stdio.
    #[command(hide = true)]
    Mcp,
    /// For programs that start their own Claude Code sessions (Workbench):
    /// the hooks, permission and MCP server to give a session in this
    /// folder, as JSON; empty when it is already wired.
    #[command(hide = true)]
    /// (The session's folder is --project, default the current one.)
    Wiring {
        /// Path of the chloectx binary to call (default: this one).
        #[arg(long)]
        exe: Option<String>,
        #[arg(long, default_value = "haiku")]
        distill_model: String,
    },
    /// Run the daemon in the foreground.
    #[command(hide = true)]
    Serve {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Search the project's memory.
    #[command(display_order = 7)]
    Search {
        query: String,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long, value_delimiter = ',')]
        kinds: Vec<String>,
    },
    /// Answer a question about earlier work
    ///
    /// The rules, notes, replies and messages that bear on it, found by
    /// search alone and packed for it.
    #[command(display_order = 6)]
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
    #[command(hide = true)]
    Sql {
        sql: String,
        /// Print `{"columns", "rows", "count", "text"}` (at most 1000 rows).
        #[arg(long)]
        json: bool,
    },
    /// Store a decision, preference, fact, todo or note.
    #[command(display_order = 8)]
    Remember { kind: String, text: String },
    /// Delete records from memory for good: this project's by id or by
    /// their text, or with --everywhere what every store holds of a fact
    ///
    /// The ids are the ones `chloectx search` and the inspector show
    /// (`session:seq`). Each record is deleted, scrubbed from the receipts
    /// that showed it, and the automatic snapshots that hold it are dropped;
    /// the other records of its turn stay. It cannot be undone.
    ///
    /// Given words that are not record ids, it forgets that text (whatever
    /// its case; 3 characters or more): the records whose own text holds it
    /// are deleted; those that hold it only in their context (the request of
    /// the turn they came from, such as a rule given in the same prompt) are
    /// kept, with it cut from that context; and the session state, the logs
    /// and every receipt lose it. What goes and what stays is listed first,
    /// each record with its kind and first line, and it asks (or --yes;
    /// --dry-run only lists). With --everywhere, every store `chloectx
    /// stores` lists, by text or by id. With a sync set up, the server's
    /// copies go too.
    #[command(hide = true)]
    Forget {
        /// Record ids, such as a1b2c3d4:42, or the text to forget.
        #[arg(required = true)]
        ids: Vec<String>,
        /// In every store on this machine, not only this project's.
        #[arg(long)]
        everywhere: bool,
        /// Forget what was listed without asking.
        #[arg(long, short)]
        yes: bool,
        /// Only list what would be forgotten.
        #[arg(long)]
        dry_run: bool,
    },
    /// The edit behind a line of code (`path:line`) or a file: its session,
    /// prompt and tool calls, the notes written from its turn, the records
    /// memory had recalled into the agent's context before it (with their
    /// scores and what became of them), and that recall replayed on the
    /// snapshot kept at the time and on today's store.
    #[command(hide = true)]
    Blame {
        target: String,
        /// Do not replay the recall.
        #[arg(long)]
        no_replay: bool,
        /// Print the JSON result.
        #[arg(long)]
        json: bool,
    },
    /// The newest receipts: one for every recall and memory search. With
    /// `on` or `off`, turns receipts and automatic snapshots on or off for
    /// this project's store (in its config.json, which wins over
    /// $CHLOECTX_RECEIPTS).
    #[command(hide = true)]
    Receipts {
        #[arg(value_parser = ["on", "off"])]
        switch: Option<String>,
        /// With `off`: also remove the receipts and automatic snapshots kept so far.
        #[arg(long, requires = "switch")]
        purge: bool,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Print the JSON result: each receipt with the rows it showed
        /// (`id`, `kind`, `tag`, `score`, `decision`, `text`).
        #[arg(long)]
        json: bool,
    },
    /// One receipt: every candidate's scores, tokens and decision, and the
    /// recall replayed on the snapshot kept at its commit and on today's store.
    #[command(hide = true)]
    Receipt {
        id: String,
        #[arg(long)]
        no_replay: bool,
        /// Also print the text the agent was shown.
        #[arg(long)]
        text: bool,
        #[arg(long)]
        json: bool,
    },
    /// Snapshots of the store kept by name, which receipts replay on
    /// (others are kept after recalls and pruned by age).
    #[command(hide = true)]
    Snapshot {
        #[command(subcommand)]
        what: SnapCmd,
    },
    /// Counts and sizes.
    #[command(hide = true)]
    Stats,
    /// Where the project's time went, from what the store recorded: the
    /// model's, the tools', waiting for you and compactions, for each
    /// session and in all; the commands that took most time, errors seen
    /// again and again, files read many times, and up to five suggestions.
    /// Reads the store and writes nothing. Times are whole seconds.
    #[command(hide = true)]
    Report {
        /// The last N days.
        #[arg(long, default_value_t = 7.0)]
        days: f64,
        /// Only this session (its id, or the start of it).
        #[arg(long)]
        session: Option<String>,
        /// Print the JSON report.
        #[arg(long)]
        json: bool,
    },
    /// What you told agents for one task in more than one session: the
    /// instructions ChloeCtx keeps as rules of every session once you say
    /// them in a second session. Reads the store and writes nothing.
    #[command(hide = true)]
    Repeats {
        /// The last N days.
        #[arg(long, default_value_t = 30)]
        days: i64,
        /// Print the JSON result: each group's rule, its number of sessions,
        /// and what was said in each (`id`, `session`, `ts`, `text`).
        #[arg(long)]
        json: bool,
        /// Read this store directory itself, read-only, beside the ChloeCtx
        /// that writes it (one too old to answer `repeats`), instead of
        /// asking the project's ChloeCtx.
        #[arg(long)]
        store: Option<PathBuf>,
        /// Ask the distiller about each case of this JSON-lines file
        /// (`{"id", "request", "answer"?, "before": [{"id", "text"}],
        /// "expect": [id]}`) and count the repeats it finds against
        /// `expect`; nothing is stored.
        #[arg(long)]
        judge: Option<PathBuf>,
        /// The distiller's model for `--judge` (otherwise
        /// `CHLOECTX_DISTILL_MODEL`, or the store's).
        #[arg(long)]
        model: Option<String>,
    },
    /// Copy the project's store to a new directory while ChloeCtx runs: a
    /// consistent copy of the database, the session state and the event
    /// log. To restore, run `chloectx stop` and copy the files back.
    #[command(hide = true)]
    Backup { dest: PathBuf },
    /// Print the session-start briefing.
    #[command(hide = true)]
    Briefing,
    /// Write down the decisions, rules, requirements and facts of the
    /// stored turns not distilled yet (a store recorded before distillation
    /// existed, a turn whose distillation failed or was cut short).
    #[command(hide = true)]
    Distill {
        /// Distill every stored turn again, those distilled already too.
        #[arg(long)]
        again: bool,
        /// Only say how many turns it would distill.
        #[arg(long)]
        dry_run: bool,
    },
    /// Merge related notes now (it also runs on its own every few turns).
    #[command(hide = true)]
    Consolidate,
    /// Redact the secrets a store recorded before redaction knew their shape:
    /// rewrites the project's records (with --all, every store's), session
    /// state and event log, compacts the store, and replaces the records on
    /// the sync's server. Prints how many changed, never the secrets.
    #[command(hide = true)]
    Redact {
        #[arg(long)]
        all: bool,
        /// Count what would change, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop the project's daemon (with --all, every one); hooks start it again
    #[command(display_order = 11)]
    Stop {
        #[arg(long)]
        all: bool,
    },
    /// Check that ChloeCtx works here, and say how to fix what does not
    #[command(display_order = 3)]
    Doctor,
    /// Install the newest version from the checkout ChloeCtx was built from
    ///
    /// Pulls the checkout (fast forward only), builds and installs it over
    /// this binary, stops the old daemons, wires the user-level hooks again
    /// (a version may add some), moves the store folder from its name
    /// before the rename, and runs doctor.
    #[command(display_order = 4)]
    Upgrade {
        /// Only say whether a newer version is there.
        #[arg(long)]
        check: bool,
        /// Build the checkout as it is, without `git pull`.
        #[arg(long)]
        no_pull: bool,
        /// Build and install even when nothing changed.
        #[arg(long)]
        force: bool,
    },
    /// Attach the memory of a project that moved to its new folder
    ///
    /// Stores are kept by the project folder's path, so a project that is
    /// moved or renamed starts with an empty memory. This makes the store of
    /// OLD the store of NEW (which must have none of its own yet).
    #[command(hide = true)]
    Move {
        /// Where the project was.
        old: PathBuf,
        /// Where it is now.
        new: PathBuf,
    },
    /// List every store on this machine: its project, size and last use
    #[command(display_order = 9)]
    Stores {
        #[arg(long)]
        json: bool,
    },
    /// Remove the stores nothing will use again
    ///
    /// Those whose project folder is gone, or is a temporary folder unused
    /// for a day, or that name no project and were unused for a month.
    /// Lists them and asks first (or pass --yes).
    #[command(display_order = 10)]
    Prune {
        /// Also remove stores not used for this many days.
        #[arg(long)]
        older_than: Option<f64>,
        /// Remove them without asking.
        #[arg(long, short)]
        yes: bool,
        /// Only list what would be removed.
        #[arg(long)]
        dry_run: bool,
    },
    /// Browse what ChloeCtx stored, in the inspector (a local web page)
    #[command(display_order = 5)]
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
    #[command(hide = true)]
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
        /// sends its CHLOEDB_APP_TOKEN); without one the app refuses to start,
        /// unless --insecure ($CHLOECTX_APP_TOKEN).
        #[arg(long, env = "CHLOECTX_APP_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Serve any caller with no token (on loopback, only requests whose
        /// Host is this machine's); without it, the app refuses to start with
        /// no token.
        #[arg(long)]
        insecure: bool,
        /// The embedder (static, or hash:<dim> for tests; $CHLOECTX_EMBEDDER).
        #[arg(long, env = "CHLOECTX_EMBEDDER", default_value = "static")]
        embedder: String,
        /// Also show this machine's Claude Code stores in the command
        /// center (Receipts and Blame pages; $CHLOECTX_APP_LOCAL=1). Their
        /// records are transcripts: turn it on only where the command
        /// center's users may read them.
        #[arg(long, env = "CHLOECTX_APP_LOCAL", value_parser = clap::builder::BoolishValueParser::new(), default_value_t = false)]
        local_stores: bool,
    },
    /// Cut a capsule from the project's memory: a read-only ChloeDB file of
    /// the selected records (default: distilled notes, entities and the
    /// conversation, no raw tool output), redacted again, with a manifest.
    /// Another agent mounts it with `chloectx mount`.
    #[command(hide = true)]
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
        /// Only the records of these sessions (Claude Code session ids,
        /// comma-separated), and the entities they link to: a branch's
        /// agents' sessions. `CHLOECTX_PACK_SESSIONS` says the same.
        #[arg(long, value_delimiter = ',')]
        sessions: Vec<String>,
    },
    /// Serve capsules to Claude Code as an MCP server on stdio
    /// (capsule_ask, capsule_search, capsule_timeline, capsule_sql).
    #[command(hide = true)]
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
    #[command(hide = true)]
    Rpc {
        request: String,
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    /// Back-fill records from a Claude Code transcript.
    #[command(hide = true)]
    IngestTranscript { path: PathBuf },
    /// Copy the project's memory to a ChloeDB server or cluster, and back.
    #[command(hide = true)]
    Sync {
        #[command(subcommand)]
        what: SyncCmd,
    },
    /// Download an embedding model (static: potion-retrieval-32M; bert: bge-small-en-v1.5).
    #[command(hide = true)]
    FetchModel {
        #[arg(default_value = "static")]
        which: String,
    },
}

#[derive(Subcommand)]
enum SnapCmd {
    /// Keep a snapshot of the store now, under NAME, until it is dropped.
    Keep { name: String },
    /// The kept snapshots.
    List,
    /// Drop a kept snapshot.
    Drop { name: String },
}

#[derive(Subcommand)]
enum SyncCmd {
    /// Name the server (e.g. https://chloedb.example.com) and create its table.
    Setup {
        #[arg(long)]
        url: String,
        /// Read the server's key (an API key or token) from stdin, kept
        /// owner-only in ChloeCtx's home as this project's own (a key may
        /// be limited to one project's table), never on the command line.
        #[arg(long, conflicts_with = "token_env")]
        token_stdin: bool,
        /// Sync every project on this machine, not only this one, and keep
        /// the key from stdin as the machine's too (sync.token), which a
        /// project without a key of its own sends: an admin's key, or one
        /// that may write several projects' tables. Without it, only this
        /// project syncs (and those set up before in their own folders).
        #[arg(long)]
        every_project: bool,
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
    /// Stop syncing this project, and remove its key; with
    /// --every-project, stop syncing every project (those set up in their
    /// own folders go on) and remove the machine's key. The server keeps
    /// what it was sent.
    Off {
        #[arg(long)]
        every_project: bool,
    },
    /// What was sent and received, and what the server holds for this
    /// project; whether it syncs, and which projects of the machine do.
    Status,
    /// Send what the store recorded since the last push.
    Push {
        /// Send every record again (the server's table lost them, or they
        /// went to another table).
        #[arg(long)]
        again: bool,
    },
    /// Push, then store the project's records the server holds and the store lacks.
    Pull,
    /// Retire a rule (or a decision, fact, to-do or note) for the whole
    /// project: here, on the sync server, and on every machine of the
    /// project as it next pulls. Whoever may write the project's table may;
    /// the server's audit log names the key.
    Retire {
        /// The record's id, as chloectx search or the briefing shows it.
        id: String,
    },
    /// The records pulled from other machines whose rules and decisions are
    /// kept out of the briefing and the prompt's context until accepted, by
    /// machine (named by the key it pushed with); with --origin or --all,
    /// accept those machines' records in this project, now and on later
    /// pulls.
    Accept {
        /// A machine as listed: its name (`ana-laptop`), its key's name, or
        /// its id (`m-44fb96669a15`). Repeatable.
        #[arg(long = "origin")]
        origins: Vec<String>,
        /// Every machine listed now, in this project only; a machine that
        /// pushes later waits to be accepted too.
        #[arg(long, conflicts_with = "origins")]
        all: bool,
    },
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
        Cmd::Wiring { exe, distill_model } => {
            let exe = exe.unwrap_or_else(|| std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "chloectx".into()));
            println!("{}", install::wiring(&project, &exe, Some(&distill_model)));
        }
        Cmd::Serve { dir } => {
            let dir = dir.unwrap_or_else(|| paths::store_dir(&project));
            // The folder the store is keyed by: a worktree's sessions reach
            // the daemon of its repository's main checkout.
            if let Err(e) = daemon::serve(&dir, &paths::store_root(&project).to_string_lossy()) {
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
        Cmd::Sql { sql, json: as_json } => {
            let r = send(&project, json!({"op": "sql", "sql": sql, "json": as_json}), 30);
            if as_json {
                println!("{}", serde_json::to_string(&r).unwrap());
            } else {
                text(&r);
            }
        }
        Cmd::Blame { target, no_replay, json: as_json } => {
            // Relative to where it is typed, not to the daemon's directory.
            let (path, line) = chloectx::blame::parse_target(&target);
            let abs = std::path::absolute(&path).unwrap_or_else(|_| path.clone().into());
            let target = match line {
                Some(n) => format!("{}:{n}", abs.display()),
                None => abs.display().to_string(),
            };
            let r = send(&project, json!({"op": "blame", "target": target, "replay": !no_replay}), 120);
            if as_json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap());
            } else {
                text(&r);
            }
        }
        Cmd::Receipts { switch: Some(s), purge, .. } => text(&send(&project, json!({"op": "receipts_switch", "on": s == "on", "purge": purge}), 600)),
        Cmd::Receipts { switch: None, session, limit, json: as_json, .. } => {
            let r = send(&project, json!({"op": "receipts", "session": session, "limit": limit}), 30);
            if as_json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap());
            } else {
                text(&r);
            }
        }
        Cmd::Receipt { id, no_replay, text: show, json: as_json } => {
            let r = send(&project, json!({"op": "receipt", "id": id, "replay": !no_replay, "text": show}), 120);
            if as_json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap());
            } else {
                text(&r);
            }
        }
        Cmd::Snapshot { what } => {
            let req = match what {
                SnapCmd::Keep { name } => json!({"op": "snapshot", "what": "keep", "name": name}),
                SnapCmd::List => json!({"op": "snapshot", "what": "list"}),
                SnapCmd::Drop { name } => json!({"op": "snapshot", "what": "drop", "name": name}),
            };
            text(&send(&project, req, 600));
        }
        // A person ran it, or Workbench on one's behalf (an answer, a rule):
        // the user's.
        Cmd::Remember { kind, text: t } => text(&send(&project, json!({"op": "remember", "kind": kind, "text": t, "by": "user", "caller": daemon::caller("cli")}), 30)),
        Cmd::Repeats { days, json: as_json, store, judge, model } => {
            let r = match (store, judge) {
                (store, Some(file)) => {
                    let mut cfg = store.map(|d| daemon::Config::load(&d.join("config.json")).distill).unwrap_or_default();
                    if let Some(m) = model {
                        std::env::remove_var("CHLOECTX_DISTILL_MODEL");
                        cfg.model = m;
                    }
                    let cases: Vec<Json> = std::fs::read_to_string(&file)
                        .unwrap_or_else(|e| {
                            eprintln!("chloectx repeats: {}: {e}", file.display());
                            std::process::exit(1)
                        })
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| {
                            serde_json::from_str(l).unwrap_or_else(|e| {
                                eprintln!("chloectx repeats: {}: a line is not JSON: {e}", file.display());
                                std::process::exit(2)
                            })
                        })
                        .collect();
                    chloectx::repeats::judge(&cfg, &cases, 4)
                }
                (Some(dir), None) => {
                    let cfg = daemon::Config::load(&dir.join("config.json"));
                    let scanned = embed::open(&cfg.embedder)
                        .and_then(|e| chloectx::store::Store::open_read_only(&dir.join("memory.strata"), std::sync::Arc::from(e)))
                        .and_then(|s| chloectx::repeats::scan(&s, chloectx::now() - days.max(1) * 86_400, 20_000));
                    scanned.unwrap_or_else(|e| {
                        eprintln!("chloectx repeats: {}: {e}", dir.display());
                        std::process::exit(1)
                    })
                }
                (None, None) => send(&project, json!({"op": "repeats", "days": days}), 120),
            };
            if as_json {
                println!("{}", serde_json::to_string_pretty(&r).unwrap());
            } else {
                text(&r);
            }
        }
        Cmd::Report { days, session, json: as_json } => {
            let r = send(&project, json!({"op": "report", "days": days, "session": session}), 120);
            if as_json {
                println!("{}", serde_json::to_string_pretty(&r["report"]).unwrap());
            } else {
                print!("{}", r["text"].as_str().unwrap_or(""));
            }
        }
        Cmd::Stats => println!("{}", serde_json::to_string_pretty(&send(&project, json!({"op": "stats"}), 30)).unwrap()),
        Cmd::Backup { dest } => {
            let dest = std::path::absolute(&dest).unwrap_or(dest);
            let r = send(&project, json!({"op": "backup", "dest": dest}), 3600);
            println!("{}: {} records, {} bytes, in {:.1} s", dest.display(), r["records"], r["bytes"], r["seconds"].as_f64().unwrap_or(0.0));
        }
        Cmd::Briefing => text(&send(&project, json!({"op": "briefing", "session": ""}), 30)),
        Cmd::Distill { again, dry_run } => {
            // What it is about to do, then what it did: each turn is a model
            // call or two, and a run can take minutes (ANA-1).
            let plan = send(&project, json!({"op": "distill_backfill", "again": again, "dry_run": true}), 120);
            let n = |k: &str| plan[k].as_u64().unwrap_or(0);
            let turns = |k: u64| format!("{k} turn{}", if k == 1 { "" } else { "s" });
            let already = if again || n("already") == 0 { String::new() } else { format!("; {} distilled already, left as they are (--again distills them again)", turns(n("already"))) };
            let failed = if n("failed_before") > 0 { format!(", {} whose distillation failed before", n("failed_before")) } else { String::new() };
            println!("{} stored: {} to distill{failed}{already}.", turns(n("turns")), turns(n("to_distill")));
            if plan["active"] == json!(false) {
                println!("Distillation is off (CHLOECTX_DISTILL=0, or distill.enabled false in config.json): nothing distilled.");
            } else if !dry_run && n("to_distill") > 0 {
                println!("Distilling: about {} model calls.", n("to_distill") * 2);
                let r = send(&project, json!({"op": "distill_backfill", "again": again}), 6 * 3600);
                println!("Distilled {} in {} s: {} new records.", turns(r["to_distill"].as_u64().unwrap_or(0)), r["seconds"].as_u64().unwrap_or(0), r["items"].as_u64().unwrap_or(0));
            }
        }
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
        Cmd::Sync { what: SyncCmd::Setup { url, token_stdin, every_project, token_env, renames, no_create } } => {
            let token = token_stdin.then(|| {
                let mut t = String::new();
                let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut t);
                t
            });
            let home = paths::home();
            let kept = chloectx::sync::setup(&home, &project.to_string_lossy(), &url, token.as_deref(), &token_env, &renames, every_project).unwrap_or_else(|e| {
                eprintln!("chloectx sync: {e}");
                std::process::exit(1);
            });
            let key = chloectx::sync::project_key(&home, &project.to_string_lossy());
            let remote = chloectx::sync::remote_for(&home, &key).expect("just set up");
            let was = chloectx::sync::saved_state(&paths::store_dir(&project));
            if was.url == remote.url && !was.key.is_empty() && was.key != key {
                println!("this project synced as {} before and syncs as {key} now: the next push sends every record to {}", was.key, chloectx::sync::table_for(&key));
            }
            if !kept.is_empty() {
                println!("the key is kept in {}", kept.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" and "));
            }
            if !no_create {
                if let Err(e) = remote.create_table(&key) {
                    eprintln!("chloectx sync: could not create the table {} on {}: {e}", chloectx::sync::table_for(&key), remote.url);
                    std::process::exit(1);
                }
            }
            // Not fatal: the key may only be in the environment of the
            // sessions to come (a cloud setup script runs before it is).
            match remote.count(&key) {
                Ok(n) => println!("sync set up with {}; this project syncs as {key} ({n} records there), with {}", remote.url, remote.key_from),
                Err(e) => {
                    let e = e.to_string();
                    println!("sync set up with {}; this project syncs as {key}, with {}; the server does not answer yet ({e}){}: the daemon tries again when it starts", remote.url, remote.key_from, chloectx::sync::key_hint(&key, &e));
                }
            }
            // Which other projects send their stores (LENA-301: every one did).
            println!("{}", chloectx::sync::scope_line(&home));
            // A running daemon syncs once it starts again (on the next hook or tool).
            let dir = paths::store_dir(&project);
            if std::os::unix::net::UnixStream::connect(paths::StorePaths::new(&dir).socket).is_ok() {
                send(&project, json!({"op": "shutdown"}), 10);
            }
        }
        Cmd::Sync { what: SyncCmd::Off { every_project } } => {
            let home = paths::home();
            let did = chloectx::sync::off(&home, &project.to_string_lossy(), every_project).unwrap_or_else(|e| {
                eprintln!("chloectx sync: {e}");
                std::process::exit(1);
            });
            if did.is_empty() {
                println!("nothing to stop: {}", chloectx::sync::scope_line(&home));
            }
            for line in did {
                println!("{line}");
            }
            println!("the server keeps what it was sent; {}", chloectx::sync::scope_line(&home));
        }
        Cmd::Sync { what: SyncCmd::Accept { origins, all } } => {
            let r = send(&project, json!({"op": "sync", "what": "accept", "origins": origins, "all": all}), 600);
            println!("{}", r["text"].as_str().unwrap_or(""));
            if r["unknown"].as_array().is_some_and(|u| !u.is_empty()) {
                std::process::exit(1);
            }
        }
        Cmd::Sync { what: SyncCmd::Retire { id } } => {
            let r = send(&project, json!({"op": "sync", "what": "retire", "id": id}), 120);
            println!("{}", r["text"].as_str().unwrap_or(""));
        }
        Cmd::Sync { what } => {
            let (w, again) = match what {
                SyncCmd::Status => ("status", false),
                SyncCmd::Push { again } => ("push", again),
                SyncCmd::Pull => ("pull", false),
                SyncCmd::Setup { .. } | SyncCmd::Accept { .. } | SyncCmd::Retire { .. } | SyncCmd::Off { .. } => unreachable!(),
            };
            let r = send(&project, json!({"op": "sync", "what": w, "again": again}), 600);
            println!("{}", serde_json::to_string_pretty(&r).unwrap());
            if let Some(w) = r["warning"].as_str() {
                eprintln!("chloectx sync: {w}");
            }
        }
        Cmd::Doctor => std::process::exit(doctor::run(&project)),
        Cmd::Upgrade { check, no_pull, force } => std::process::exit(chloectx::version::upgrade(&project, &chloectx::version::UpgradeOptions { check, no_pull, force })),
        Cmd::Move { old, new } => match chloectx::stores::relink(&paths::home(), &old, &new) {
            Ok(t) => println!("{t}"),
            Err(e) => {
                eprintln!("chloectx move: {e}");
                std::process::exit(1);
            }
        },
        Cmd::Forget { ids, everywhere: true, yes, dry_run } => std::process::exit(chloectx::stores::forget_everywhere(&paths::home(), &ids, &chloectx::stores::ForgetOptions { yes, dry_run })),
        Cmd::Forget { ids, yes, dry_run, .. } => {
            // Only a store that exists: forgetting never creates one.
            let dir = paths::store_dir(&project);
            if !dir.join("memory.strata").exists() {
                eprintln!("chloectx forget: {} has no store ({}); run it in the project's folder, or pass --project", project.display(), dir.display());
                std::process::exit(1);
            }
            // A text, or a list first: listed, and asked.
            if dry_run || !ids.iter().all(|i| chloectx::stores::is_record_id(i.trim())) {
                std::process::exit(chloectx::stores::forget_in(&paths::home(), Some(&dir), &ids, &chloectx::stores::ForgetOptions { yes, dry_run }));
            }
            let mut missing = 0;
            for id in &ids {
                let r = send(&project, json!({"op": "forget", "id": id, "wait": true}), 120);
                let t = r["text"].as_str().unwrap_or("");
                println!("{t}");
                missing += usize::from(t.starts_with("No record"));
            }
            if missing > 0 {
                std::process::exit(1);
            }
        }
        Cmd::Stores { json: as_json } => std::process::exit(chloectx::stores::list(&paths::home(), as_json)),
        Cmd::Prune { older_than, yes, dry_run } => std::process::exit(chloectx::stores::prune(&paths::home(), &chloectx::stores::PruneOptions { older_than_days: older_than, yes, dry_run })),
        Cmd::Pack { out, kinds, since_days, max_records, title, json: as_json, sessions } => {
            let mut sessions: Vec<String> = sessions.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            if sessions.is_empty() {
                sessions = std::env::var("CHLOECTX_PACK_SESSIONS").unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
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
            // A consistent copy of the store, which no daemon writes while
            // it is cut, in a directory only this user can open: it holds
            // the whole store, its event log and session state.
            let tmp = paths::private_temp_dir("chloectx-pack").unwrap_or_else(|e| {
                eprintln!("chloectx pack: a private temporary directory: {e}");
                std::process::exit(1)
            });
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
            let opts = chloectx::capsule::PackOptions { kinds, since_ts: since_days.map(|d| chloectx::now() - (d * 86_400.0) as i64), max_records, title, sessions };
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
        Cmd::App { server, key, host, port, token, insecure, embedder, local_stores } => {
            let local = local_stores.then(paths::home);
            if let Err(e) = chloectx::app::run(chloectx::app::Options { host, port, server, key, token: token.filter(|t| !t.is_empty()), insecure, embedder, local }) {
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
                // One store at a time, and a daemon started for it here is
                // stopped after it (CTX-8: `--all` left one per store).
                let was_running = std::os::unix::net::UnixStream::connect(paths::StorePaths::new(&dir).socket).is_ok();
                let res = redact_store(&dir, &proj, dry_run);
                if !was_running {
                    let _ = daemon::request_running(&dir, &json!({"op": "shutdown"}), Duration::from_secs(10));
                }
                match res {
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
            // Only a daemon that runs: stopping never starts one, nor
            // creates a store (CTX-13).
            let dir = paths::store_dir(&project);
            match daemon::request_running(&dir, &json!({"op": "shutdown"}), Duration::from_secs(10)) {
                Ok(_) => println!("stopped the daemon of {}; it starts again when a hook or tool needs it", project.display()),
                Err(_) => println!("no daemon runs for {}", project.display()),
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Every command is either in `--help` or named in [`MORE`].
    #[test]
    fn every_command_is_listed_somewhere() {
        let cmd = Cli::command();
        let mut visible = Vec::new();
        for c in cmd.get_subcommands() {
            let name = c.get_name();
            if c.is_hide_set() {
                assert!(MORE.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-')).any(|w| w == name), "`{name}` is hidden from --help and not named in MORE");
            } else {
                visible.push((c.get_display_order(), name.to_string()));
            }
        }
        visible.sort();
        let visible: Vec<String> = visible.into_iter().map(|v| v.1).collect();
        assert_eq!(visible, ["install", "uninstall", "doctor", "upgrade", "ui", "ask", "search", "remember", "stores", "prune", "stop"]);
    }
}
