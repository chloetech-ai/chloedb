//! Copies a project's memory to a ChloeDB server or cluster, and back.
//!
//! The store stays a local file, so that hooks and recall never wait on the
//! network. The daemon pushes the records it has not sent yet to its
//! project's table ([`table_for`], `chloectx_mem_<project>`) on the server
//! set up with `chloectx sync setup --url URL` every [`PUSH_EVERY`] and as
//! it exits, and when it starts it pulls the records of its project the
//! store lacks, which a store made on another machine (or a cloud
//! container started afresh) wrote, and embeds them here. A project is its
//! git repository (`owner/name`, see [`project_key`]), so every checkout
//! of it, under any folder or an old name, shares one memory.
//!
//! A project syncs once `chloectx sync setup` ran in its folder (it is in
//! `sync.json`'s `projects`, or has a key of its own), or when the machine
//! was set up for every project (`--every-project`, or `CHLOECTX_SYNC_URL`
//! in the environment): setting up one project uploads no other's store
//! (LENA-301, audit CTX-34). See [`syncs`].
//!
//! A table per project is the boundary: a key limited to one project's
//! table (`{"chloectx_mem_<project>": "write"}`, which the Inspector makes
//! per machine) reads and writes that project only. Before 0.3.4 every
//! project shared one table, [`TABLE`] (`chloectx_memory`), which any sync
//! key read all of; its rows of the project are still pulled where the
//! key may read them, and nothing is pushed there.
//!
//! Each row names the machine that pushed it (`origin`, [`origin`]). A
//! pulled record from another machine is linked `synced_from` it, and its
//! rules and decisions stay out of the briefing and the prompt's context
//! until a person accepts that machine (`chloectx sync accept`,
//! [`pending`]): a key that may write the table could otherwise plant a
//! "user preference" that every other machine shows as its own.
//!
//! What is sent is what the store holds: text from the sessions it
//! recorded, which can include whatever passed through them. It is
//! redacted again as it is sent and as it is pulled ([`crate::redact`]),
//! for stores written before redaction knew a secret's shape. The server
//! should take it over HTTPS and need a key (an API key with the `write`
//! scope is enough once the table exists; creating it needs `admin`).
//!
//! Records retagged after they were pushed are not sent again: the server
//! keeps the version it was sent first. Records that `chloectx redact`
//! rewrites are ([`push_records`]). A record forgotten here has its row
//! deleted on the server, and a pull never stores it again
//! ([`Forgotten`], audit CTX-33).

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use chloedb::query::Params;

use crate::record::{Kind, Link, Record, COLUMNS};
use crate::store::Store;
use crate::{err, Result};

/// How often the daemon pushes what it recorded since.
pub const PUSH_EVERY: Duration = Duration::from_secs(30);
/// Records per request.
const BATCH: usize = 500;
/// A request's deadline.
const TIMEOUT_SECS: u64 = 30;

/// The table every project shared before 0.3.4 (pulled from, where the
/// key reads it; never written).
pub const TABLE: &str = "chloectx_memory";
/// The prefix of the projects' tables ([`table_for`]).
pub const PREFIX: &str = "chloectx_mem_";

/// The table [`TABLE`], as `chloectx sync setup` created it.
pub const CREATE: &str = "CREATE TABLE IF NOT EXISTS chloectx_memory (id KEYWORD PRIMARY KEY, project KEYWORD, rid KEYWORD, session KEYWORD, kind KEYWORD, tag KEYWORD, turn INT, seq INT, ts INT, importance FLOAT, files TEXT, content TEXT, context TEXT, links_json TEXT)";
const REMOTE_COLUMNS: &str = "rid, session, kind, tag, turn, seq, ts, importance, files, content, context, links_json";
/// A project's table adds who pushed each row.
const PROJECT_COLUMNS: &str = "rid, session, kind, tag, turn, seq, ts, importance, files, content, context, links_json, origin";

/// The table of project `project` (its [`project_key`]): `chloectx_mem_`,
/// then the key in lowercase letters, digits and `_` (at most 40), then 8
/// hex digits of its hash, so that two keys that read the same there
/// (`acme/web-app`, `acme/web_app`) are two tables.
pub fn table_for(project: &str) -> String {
    let mut slug = String::new();
    for c in project.trim().to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '_' };
        if c == '_' && (slug.is_empty() || slug.ends_with('_')) {
            continue;
        }
        slug.push(c);
    }
    let slug: String = slug.trim_end_matches('_').chars().take(40).collect();
    format!("{PREFIX}{}_{:08x}", slug.trim_end_matches('_'), fnv(project.trim()) as u32)
}

/// Project `project`'s table, as the setup (an admin's key) creates it.
pub fn create_for(project: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {} (id KEYWORD PRIMARY KEY, project KEYWORD, rid KEYWORD, session KEYWORD, kind KEYWORD, tag KEYWORD, turn INT, seq INT, ts INT, importance FLOAT, files TEXT, content TEXT, context TEXT, links_json TEXT, origin KEYWORD)",
        table_for(project)
    )
}

/// The label of the link a record pulled from another machine carries to
/// that machine's [`origin`], until it is accepted ([`ACCEPTED`]).
pub const SYNCED_FROM: &str = "synced_from";
/// The label once a person accepted the machine (`chloectx sync accept`).
pub const ACCEPTED: &str = "accepted_from";
/// The label of the link a row carries to the name of the key that pushed
/// it, as the server told the pushing machine (`GET /v1/whoami`): the
/// machine's name in `chloectx sync accept`, the inspector and the command
/// center, rather than its random [`origin`] (WEI-308, WEI-303). The key's
/// name is what an admin gave it; the command center's "Make a sync key"
/// names it `chloectx-sync-<whose machine>-<table>` ([`machine_name`]).
pub const PUSHED_BY: &str = "pushed_by";

/// The name of the key that pushed `r`, when its row said ([`PUSHED_BY`]).
pub fn pushed_by(r: &Record) -> Option<&str> {
    r.links.iter().find(|l| l.label == PUSHED_BY).map(|l| l.to.as_str()).filter(|n| !n.is_empty())
}

/// The machine a key's name names: `ana-laptop` of the command center's
/// `chloectx-sync-ana-laptop-2f73d8c1`; any other name as it is.
pub fn machine_name(key: &str) -> &str {
    let k = key.trim();
    let Some(rest) = k.strip_prefix("chloectx-sync-") else { return k };
    match rest.rsplit_once('-') {
        Some((who, hex)) if !who.is_empty() && hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit()) => who,
        _ => rest,
    }
}

/// How a pulled record's machine reads: `ana-laptop (m-44fb96669a15)`
/// when the row named its key, else the machine's id.
pub fn machine_label(origin: &str, key: Option<&str>) -> String {
    match key {
        Some(k) => format!("{} ({origin})", machine_name(k)),
        None => origin.to_string(),
    }
}

/// This machine, as the rows it pushes name it: `$CHLOECTX_SYNC_ORIGIN`
/// (a cloud environment started afresh each time sets one), else a random
/// id kept in `sync-origin` in ChloeCtx's home, made on first use.
pub fn origin(home: &Path) -> String {
    if let Some(o) = std::env::var("CHLOECTX_SYNC_ORIGIN").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        return o;
    }
    let p = home.join("sync-origin");
    if let Some(o) = std::fs::read_to_string(&p).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        return o;
    }
    let mut b = [0u8; 6];
    let _ = getrandom(&mut b);
    let o = format!("m-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>());
    let _ = crate::paths::create_private_dir(home);
    let _ = std::fs::write(&p, &o);
    o
}

fn getrandom(b: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(b)
}

/// Whether `r` came from another machine whose records were not accepted
/// yet: its rules and decisions stay out of the briefing, the prompt's
/// context and searches until then ([`held_back`]); `chloectx sync
/// accept` lists them.
pub fn pending(r: &Record) -> bool {
    r.links.iter().any(|l| l.label == SYNCED_FROM)
}

/// Whether `r` is a rule or decision from a machine not accepted yet,
/// which the briefing and the prompt's context leave out ([`pending`]).
pub fn held_back(r: &Record) -> bool {
    matches!(r.kind, Kind::Preference | Kind::Decision) && pending(r)
}

/// The machine a pending record came from.
pub fn pending_origin(r: &Record) -> Option<&str> {
    r.links.iter().find(|l| l.label == SYNCED_FROM).map(|l| l.to.as_str())
}

/// Where the sync is set up: `sync.json` (the URL, the projects that
/// sync), and the keys, readable by their owner only, in ChloeCtx's home:
/// each project's own ([`project_token_path`]) and `sync.token`, the
/// machine's, which `--every-project` keeps for the projects without one.
pub fn config_path(home: &Path) -> PathBuf {
    home.join("sync.json")
}

pub fn token_path(home: &Path) -> PathBuf {
    home.join("sync.token")
}

/// The key of project `project` (its [`project_key`]): a key is limited to
/// one project's table, so each project of a machine keeps its own, named
/// for that table, under `sync-keys/`.
pub fn project_token_path(home: &Path, project: &str) -> PathBuf {
    home.join("sync-keys").join(format!("{}.token", table_for(project)))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncConfig {
    pub url: String,
    /// The environment variable that holds the key (e.g. one a cloud
    /// environment sets), read when the daemon syncs: the key is kept in
    /// no file.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token_env: String,
    /// Repositories renamed (`owner/old` to `owner/new`): a checkout of
    /// either name syncs as the new one.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub renames: std::collections::BTreeMap<String, String>,
    /// Machines ([`origin`]) whose records a pull of any project took as
    /// accepted, from before 0.3.9, which accepted a machine for every
    /// project. Still read; `*` (what `accept --all` wrote, trusting every
    /// machine to come, for every project: WEI-308) no longer is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted: Vec<String>,
    /// Each project's accepted machines (`chloectx sync accept`), by the
    /// name it syncs as ([`project_key`]): a pull of that project takes
    /// their records as accepted, and of no other.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub accepted: std::collections::BTreeMap<String, Vec<String>>,
    /// The projects ([`project_key`]) `chloectx sync setup` ran in: only
    /// they sync, unless `every_project` (LENA-301).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
    /// `chloectx sync setup --every-project`: every project on the machine
    /// syncs, and sends the machine's key (`sync.token`) when it has none
    /// of its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub every_project: bool,
}

impl SyncConfig {
    /// Whether a pull of `project` takes the records of machine `origin`
    /// as accepted.
    pub fn trusts(&self, project: &str, origin: &str) -> bool {
        self.accepted.get(project).is_some_and(|o| o.iter().any(|x| x == origin)) || self.trusted.iter().any(|t| t != "*" && t == origin)
    }

    /// Whether `sync.json` still says `*` (every machine), which is no
    /// longer honoured.
    pub fn trusted_everyone(&self) -> bool {
        self.trusted.iter().any(|t| t == "*")
    }
}

fn save_config(home: &Path, cfg: &SyncConfig) -> Result<()> {
    crate::paths::create_private_dir(home)?;
    let p = config_path(home);
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(cfg).unwrap_or_default())?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

pub fn load_config(home: &Path) -> SyncConfig {
    std::fs::read_to_string(config_path(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// The repository `owner/name` a git remote URL names (https, ssh or a
/// proxy's `.../git/owner/name`), lowercased, without `.git`.
fn repo_of(url: &str) -> Option<String> {
    let path = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = path.rsplit_once(':').filter(|(h, _)| !h.contains("//")).map_or(path, |(_, p)| p);
    let mut parts = path.rsplit('/').filter(|p| !p.is_empty());
    let name = parts.next()?;
    let owner = parts.next()?;
    (!owner.contains('@') && !owner.contains(':')).then(|| format!("{owner}/{name}").to_lowercase())
}

/// What the project at `project` syncs as: its git repository
/// (`owner/name` of `origin`, renames applied), so that checkouts in
/// different folders, or of the repository under an old name, share their
/// memory; else its path.
pub fn project_key(home: &Path, project: &str) -> String {
    let cfg = load_config(home);
    let repo = Command::new("git").args(["-C", project, "remote", "get-url", "origin"]).stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).and_then(|o| repo_of(&String::from_utf8_lossy(&o.stdout)));
    match repo {
        Some(r) => cfg.renames.get(&r).cloned().unwrap_or(r),
        None => project.to_string(),
    }
}

fn env_var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty()).map(|t| t.trim().to_string())
}

/// Whether a server is set up on this machine at all (`$CHLOECTX_SYNC_URL`
/// or `sync.json`), whichever projects sync with it ([`syncs`]), and the
/// machine's key when it was set up for every project. `None` when no URL
/// is set.
pub fn remote(home: &Path) -> Option<Remote> {
    remote_with(home, None)
}

/// The server to sync project `project` (its [`project_key`]) with, when
/// it syncs ([`syncs`]): `$CHLOECTX_SYNC_URL`, else `sync.json`; the key:
/// `$CHLOECTX_SYNC_TOKEN`, else the variable `sync.json` names, else the
/// project's own ([`project_token_path`]), else, when the machine was set
/// up for every project, the machine's, `sync.token` (none: the server is
/// open, or a proxy adds it). `None` when no URL is set or the project
/// does not sync.
pub fn remote_for(home: &Path, project: &str) -> Option<Remote> {
    syncs(home, project)?;
    remote_with(home, Some(project))
}

/// Why project `project` (its [`project_key`]) syncs, or `None` when it
/// does not: `chloectx sync setup` ran in its folder (it is in `sync.json`'s
/// `projects`, or has a key of its own from a setup before they were
/// listed), or the machine syncs every project (`--every-project`, or
/// `$CHLOECTX_SYNC_URL`, which a cloud environment sets for all it runs).
/// A setup in one project made every other one upload its store with the
/// first project's key (LENA-301).
pub fn syncs(home: &Path, project: &str) -> Option<&'static str> {
    if env_var("CHLOECTX_SYNC_URL").is_some() {
        return Some("$CHLOECTX_SYNC_URL is set: every project this environment runs syncs");
    }
    let cfg = load_config(home);
    if cfg.url.trim().is_empty() {
        return None;
    }
    if cfg.every_project {
        Some("set up for every project on this machine (--every-project)")
    } else if cfg.projects.iter().any(|p| p == project) || project_token_path(home, project).exists() {
        Some("set up in this project")
    } else {
        None
    }
}

/// The projects set up to sync on this machine ([`syncs`]), by name; a key
/// kept before projects were listed is named by its table. Empty when the
/// machine syncs every project.
pub fn set_up_projects(home: &Path) -> Vec<String> {
    let cfg = load_config(home);
    let mut out = cfg.projects.clone();
    let listed: std::collections::HashSet<String> = out.iter().map(|p| table_for(p)).collect();
    let mut tables: Vec<String> = std::fs::read_dir(home.join("sync-keys"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".token").map(String::from))
        .filter(|t| !listed.contains(t))
        .collect();
    tables.sort();
    out.extend(tables.into_iter().map(|t| format!("the project of table {t}")));
    out
}

/// One line on which projects of this machine sync, for `setup`, `sync
/// status` and doctor.
pub fn scope_line(home: &Path) -> String {
    if env_var("CHLOECTX_SYNC_URL").is_some() {
        return "every project this environment runs syncs ($CHLOECTX_SYNC_URL is set)".into();
    }
    if load_config(home).every_project {
        return "every project on this machine syncs (set up with --every-project; `chloectx sync off --every-project` stops that)".into();
    }
    let ps = set_up_projects(home);
    match ps.len() {
        0 => "no project on this machine syncs yet".into(),
        1 => format!("only {} syncs on this machine; another project syncs once `chloectx sync setup` runs in its folder", ps[0]),
        n => format!("{n} projects sync on this machine: {}; another syncs once `chloectx sync setup` runs in its folder", ps.join(", ")),
    }
}

/// What a store that does not sync now sent to `url` before (`st`, its
/// saved state), and how it leaves the server: empty when nothing.
pub fn sent_before(st: &SyncState, url: &str) -> String {
    if st.pushed == 0 || st.url != url {
        return String::new();
    }
    let table = if st.key.is_empty() { "its table".to_string() } else { table_for(&st.key) };
    format!("; it sent {} record{} to {url} before, into {table}, which keeps them until an admin removes them there (DROP TABLE {table})", st.pushed, if st.pushed == 1 { "" } else { "s" })
}

fn remote_with(home: &Path, project: Option<&str>) -> Option<Remote> {
    let cfg = load_config(home);
    let file = |p: PathBuf| std::fs::read_to_string(p).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let url = env_var("CHLOECTX_SYNC_URL").or_else(|| Some(cfg.url.clone()).filter(|u| !u.trim().is_empty()))?;
    let named = Some(cfg.token_env.as_str()).filter(|k| !k.is_empty());
    let (token, key_from) = if let Some(t) = env_var("CHLOECTX_SYNC_TOKEN") {
        (Some(t), "$CHLOECTX_SYNC_TOKEN".to_string())
    } else if let Some((k, t)) = named.and_then(|k| env_var(k).map(|t| (k, t))) {
        (Some(t), format!("${k}"))
    } else if let Some(t) = project.and_then(|p| file(project_token_path(home, p))) {
        (Some(t), "the project's key".to_string())
    } else if let Some(t) = Some(token_path(home)).filter(|_| cfg.every_project).and_then(file) {
        // Only for every project: a key an earlier setup kept as the
        // machine's (the first one set up, LENA-301) is sent for none.
        (Some(t), "the machine's key (sync.token)".to_string())
    } else {
        (None, "no key".to_string())
    };
    Some(Remote { url: url.trim().trim_end_matches('/').to_string(), token, key_from, header_dir: home.to_path_buf() })
}

/// What to do when the server refused the key (`error`, a 403) for project
/// `project`: a key of its own. Empty for any other error.
pub fn key_hint(project: &str, error: &str) -> String {
    if !error.contains(": 403:") {
        return String::new();
    }
    format!(
        "; this project needs a key that may write {}: \"Make a sync key\" on the server's ChloeCtx page, for project {project}, then run the setup line it shows in this project's folder",
        table_for(project)
    )
}

/// Refuses a server reached over plain HTTP anywhere but this machine:
/// what sync sends is transcripts of the user's sessions, and with them the
/// server's key. `CHLOECTX_SYNC_ALLOW_HTTP=1` allows it (a private network
/// behind a proxy the user trusts).
pub fn check_url(url: &str) -> Result<()> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    let Some(rest) = lower.strip_prefix("http://") else {
        return Err(err(format!("sync server {u}: give an https:// URL")));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host.strip_prefix('[') { v6.split(']').next().unwrap_or("") } else { host.split(':').next().unwrap_or("") };
    if crate::app::loopback(host) || std::env::var("CHLOECTX_SYNC_ALLOW_HTTP").is_ok_and(|v| v == "1") {
        return Ok(());
    }
    Err(err(format!("sync server {u}: plain HTTP to another machine would send your sessions and the key in the clear; use https:// (or set CHLOECTX_SYNC_ALLOW_HTTP=1)")))
}

/// Writes the sync's settings, and the key, owner-only, when given. The
/// project in folder `folder` (by its [`project_key`]) syncs from now on,
/// and the key is its own; with `every_project` every project on the
/// machine syncs, and the key is the machine's too (`sync.token`, for the
/// projects without one). No other project syncs because of it (LENA-301:
/// the first key set up was the machine's, and every project's daemon
/// uploaded its store with it). Where the key went.
pub fn setup(home: &Path, folder: &str, url: &str, token: Option<&str>, token_env: &str, renames: &[String], every_project: bool) -> Result<Vec<PathBuf>> {
    check_url(url)?;
    crate::paths::create_private_dir(home)?;
    let mut map = std::collections::BTreeMap::new();
    for r in renames {
        let (old, new) = r.split_once('=').ok_or_else(|| err(format!("--rename {r}: give OLD=NEW, as owner/old=owner/new")))?;
        map.insert(old.trim().to_lowercase(), new.trim().to_lowercase());
    }
    let was = load_config(home);
    let mut cfg = SyncConfig { url: url.trim().to_string(), token_env: token_env.trim().to_string(), renames: map, trusted: was.trusted, accepted: was.accepted, projects: was.projects, every_project: was.every_project || every_project };
    // Written first: the project's name ([`project_key`]) reads the renames from it.
    std::fs::write(config_path(home), serde_json::to_string_pretty(&cfg).unwrap_or_default())?;
    let project = project_key(home, folder);
    if !cfg.projects.contains(&project) {
        cfg.projects.push(project.clone());
        std::fs::write(config_path(home), serde_json::to_string_pretty(&cfg).unwrap_or_default())?;
    }
    let mut kept = Vec::new();
    if let Some(t) = token.map(str::trim).filter(|t| !t.is_empty()) {
        // The project's key is the one its daemon sends; the machine's is
        // for the projects without one, kept only when every project syncs.
        let own = project_token_path(home, &project);
        crate::paths::create_private_dir(own.parent().expect("under sync-keys"))?;
        let mut paths = vec![own];
        if every_project {
            paths.push(token_path(home));
        }
        for p in paths {
            let tmp = p.with_extension("tmp");
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(t.as_bytes())?;
            drop(f);
            std::fs::rename(&tmp, &p)?;
            kept.push(p);
        }
    }
    Ok(kept)
}

/// `chloectx sync off`: the project in folder `folder` stops syncing, and
/// its own key is removed; with `every_project`, the machine stops syncing
/// every project (the projects set up in their folders go on) and its key,
/// `sync.token`, is removed. What it did, one line each. The server keeps
/// what it was sent.
pub fn off(home: &Path, folder: &str, every_project: bool) -> Result<Vec<String>> {
    let mut cfg = load_config(home);
    let mut did = Vec::new();
    if every_project {
        if cfg.every_project {
            cfg.every_project = false;
            did.push("the machine no longer syncs every project".to_string());
        }
        if token_path(home).exists() {
            std::fs::remove_file(token_path(home))?;
            did.push(format!("removed the machine's key, {}", token_path(home).display()));
        }
    } else {
        let project = project_key(home, folder);
        let before = cfg.projects.len();
        cfg.projects.retain(|p| *p != project);
        let own = project_token_path(home, &project);
        if own.exists() {
            std::fs::remove_file(&own)?;
            did.push(format!("removed this project's key, {}", own.display()));
        }
        if before != cfg.projects.len() || !did.is_empty() {
            did.insert(0, format!("{project} no longer syncs"));
        }
        if cfg.every_project {
            did.push("the machine still syncs every project (`chloectx sync off --every-project` stops that)".into());
        }
    }
    if config_path(home).exists() {
        std::fs::write(config_path(home), serde_json::to_string_pretty(&cfg).unwrap_or_default())?;
    }
    Ok(did)
}

/// A ChloeDB server's HTTP API, called with `curl`.
pub struct Remote {
    pub url: String,
    token: Option<String>,
    /// Where the key came from, for `chloectx sync status` and doctor.
    pub key_from: String,
    /// Where the request's header file is written (owner-only, removed
    /// after): the key is never on a command line.
    header_dir: PathBuf,
}

impl Remote {
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// POSTs `body` to `route`; the answer's JSON, or an error with the
    /// server's message.
    fn post(&self, route: &str, body: &Json) -> Result<Json> {
        self.call("POST", route, Some(body))
    }

    /// The name of the key this machine sends, as the server knows it
    /// (`GET /v1/whoami`): what the rows it pushes say pushed them
    /// ([`PUSHED_BY`]). `None` for the server's own token or an open
    /// server, which name no one, and when the server does not answer.
    pub fn key_name(&self) -> Option<String> {
        let j = self.call("GET", "/v1/whoami", None).ok()?;
        let name = j.get("name").and_then(|v| v.as_str())?.trim();
        (j.get("key").is_some_and(|k| !k.is_null()) && !name.is_empty()).then(|| name.to_string())
    }

    fn call(&self, method: &str, route: &str, body: Option<&Json>) -> Result<Json> {
        if self.token.as_deref().is_some_and(is_anthropic_key) {
            return Err(err(format!("the sync's key is an Anthropic API key (sk-ant-...), not a ChloeDB token or API key (sk_<id>_<secret>): it was not sent to {}. It came from {}: put the server's key there instead", self.url, self.key_from)));
        }
        check_url(&self.url)?;
        let mut cmd = Command::new("curl");
        cmd.args(["-sS", "-m", &TIMEOUT_SECS.to_string(), "-X", method, "-H", "x-chloedb-client: chloectx-sync", "-w", "\n%{http_code}"]);
        if body.is_some() {
            cmd.args(["-H", "content-type: application/json", "--data-binary", "@-"]);
        }
        let header = match &self.token {
            Some(t) => {
                // A file of this request's own: a push and an explicit `sync`
                // may run at once, and neither may remove the other's.
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let p = self.header_dir.join(format!(".sync-header-{}-{n}", std::process::id()));
                let _ = std::fs::remove_file(&p);
                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p)?;
                writeln!(f, "Authorization: Bearer {t}")?;
                cmd.arg("-H").arg(format!("@{}", p.display()));
                Some(p)
            }
            None => None,
        };
        cmd.arg(format!("{}{route}", self.url));
        let out = (|| {
            let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| err(format!("curl: {e}")))?;
            let mut stdin = child.stdin.take().expect("stdin");
            if let Some(b) = body {
                stdin.write_all(b.to_string().as_bytes())?;
            }
            drop(stdin);
            child.wait_with_output().map_err(|e| err(format!("curl: {e}")))
        })();
        if let Some(p) = header {
            let _ = std::fs::remove_file(p);
        }
        let out = out?;
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
        let code: u16 = code.trim().parse().unwrap_or(0);
        if code == 0 {
            return Err(err(format!("{}: {}", self.url, String::from_utf8_lossy(&out.stderr).trim())));
        }
        let j: Json = serde_json::from_str(body).unwrap_or_else(|_| json!({"error": body.chars().take(300).collect::<String>()}));
        if !(200..300).contains(&code) {
            let msg = j.get("error").and_then(|e| e.as_str()).unwrap_or(body);
            return Err(err(format!("{}{route}: {code}: {msg}", self.url)));
        }
        Ok(j)
    }

    /// Creates project `project`'s table (once; needs the `admin` scope).
    pub fn create_table(&self, project: &str) -> Result<()> {
        self.post("/v1/sql", &json!({ "sql": create_for(project) })).map(|_| ())
    }

    fn rows(&self, sql: &str, params: Json) -> Result<Vec<Vec<Json>>> {
        let j = self.post("/v1/query", &json!({ "sql": sql, "params": params }))?;
        Ok(j.get("rows").and_then(|r| r.as_array()).map(|rs| rs.iter().filter_map(|r| r.as_array().cloned()).collect()).unwrap_or_default())
    }

    /// How many records of `project` the server holds.
    pub fn count(&self, project: &str) -> Result<i64> {
        let rows = self.rows(&format!("SELECT COUNT(*) FROM {} WHERE project = :p", table_for(project)), json!({ "p": project }))?;
        Ok(rows.first().and_then(|r| r.first()).and_then(|v| v.as_i64()).unwrap_or(0))
    }
}

/// What was pushed, per project store: `sync-state.json` beside the store.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncState {
    /// The server these marks are for: another URL starts again.
    pub url: String,
    /// The name the project synced as ([`project_key`]), whose table the
    /// marks are for: another name (an origin added, a rename) is another
    /// table, which starts again from the first record (ANA-3: 318 records
    /// were "pushed" and none reached the new table). Empty in a state
    /// written before it was kept: taken for the current one.
    #[serde(default)]
    pub key: String,
    /// Every record up to this sequence number was sent.
    pub pushed_seq: i64,
    pub last_push: i64,
    pub last_pull: i64,
    pub pushed: u64,
    pub pulled: u64,
    pub last_error: String,
}

fn state_path(store_dir: &Path) -> PathBuf {
    store_dir.join("sync-state.json")
}

pub fn load_state(store_dir: &Path, url: &str, key: &str) -> SyncState {
    let s = saved_state(store_dir);
    if s.url == url && (s.key.is_empty() || s.key == key) {
        SyncState { key: key.to_string(), ..s }
    } else {
        SyncState { url: url.to_string(), key: key.to_string(), ..SyncState::default() }
    }
}

/// The state as saved, for whichever server and name.
pub fn saved_state(store_dir: &Path) -> SyncState {
    std::fs::read_to_string(state_path(store_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// What `chloectx sync status` says is wrong when the server holds fewer of
/// the project's records than this store has sent or taken from it (the
/// records up to `pushed_seq`, `upto`): a table that lost them, or marks
/// for another table.
pub fn short_on_server(upto: u64, on_server: i64) -> Option<String> {
    (on_server >= 0 && upto > on_server as u64).then(|| {
        format!("the server holds {on_server} of this project's records, fewer than the {upto} this store sent or took from it: `chloectx sync push --again` sends them all again")
    })
}

pub fn save_state(store_dir: &Path, s: &SyncState) {
    let tmp = state_path(store_dir).with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(s).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, state_path(store_dir));
    }
}

/// The records forgotten in a store (audit CTX-33): `sync-forgotten.json`
/// beside it. A pull never stores one of them again, whichever server or
/// machine still holds it, and each push deletes the server's rows of
/// those whose delete the server has not answered yet.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Forgotten {
    /// Every record forgotten here, by id.
    #[serde(default)]
    pub ids: std::collections::BTreeSet<String>,
    /// Those whose row the sync server may still hold.
    #[serde(default)]
    pub unsent: std::collections::BTreeSet<String>,
}

/// Held while `sync-forgotten.json` is read and written again: a forget
/// adds to it while a push takes from it.
static FORGOTTEN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn forgotten_path(store_dir: &Path) -> PathBuf {
    store_dir.join("sync-forgotten.json")
}

pub fn load_forgotten(store_dir: &Path) -> Forgotten {
    std::fs::read_to_string(forgotten_path(store_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_forgotten(store_dir: &Path, f: &Forgotten) -> Result<()> {
    let tmp = forgotten_path(store_dir).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(f).unwrap_or_default())?;
    std::fs::rename(tmp, forgotten_path(store_dir))?;
    Ok(())
}

/// Notes `ids` as forgotten in the store at `store_dir`: never pulled again,
/// and their server rows to delete ([`send_forgets`]).
pub fn forget(store_dir: &Path, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let _one = FORGOTTEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = load_forgotten(store_dir);
    for id in ids {
        f.ids.insert(id.clone());
        f.unsent.insert(id.clone());
    }
    save_forgotten(store_dir, &f)
}

/// Deletes the server's rows of the records forgotten in the store at
/// `store_dir` that it may still hold: in the project's table, and in the
/// table projects shared before 0.3.4 where the key may write it (a key of
/// the project's table alone may not, and the store's list keeps those
/// from coming back here all the same). How many were sent.
pub fn send_forgets(remote: &Remote, project: &str, store_dir: &Path) -> Result<usize> {
    let unsent: Vec<String> = {
        let _one = FORGOTTEN.lock().unwrap_or_else(|e| e.into_inner());
        load_forgotten(store_dir).unsent.into_iter().collect()
    };
    if unsent.is_empty() {
        return Ok(0);
    }
    let table = table_for(project);
    for batch in unsent.chunks(BATCH) {
        let ops = |t: &str| -> Vec<Json> { batch.iter().map(|id| json!({"op": "delete", "table": t, "key": format!("{project}:{id}")})).collect() };
        match remote.post("/v1/write", &json!({ "ops": ops(&table) })) {
            // A server with no table of the project holds none of its rows.
            Err(e) if no_table(&e, &table) => {}
            r => drop(r?),
        }
        let _ = remote.post("/v1/write", &json!({ "ops": ops(TABLE) }));
    }
    let _one = FORGOTTEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = load_forgotten(store_dir);
    for id in &unsent {
        f.unsent.remove(id);
    }
    save_forgotten(store_dir, &f)?;
    Ok(unsent.len())
}

/// A record as the server's row, redacted, pushed by machine `origin`
/// with the key named `by` ([`PUSHED_BY`]). A record this store pulled
/// keeps the machine it came from, and the key that pushed it.
fn remote_doc(project: &str, origin: &str, by: Option<&str>, r: &Record) -> Json {
    let mut r = r.clone();
    crate::redact::redact_record(&mut r);
    let from = r.links.iter().find(|l| l.label == SYNCED_FROM || l.label == ACCEPTED).map(|l| l.to.clone());
    if let (None, Some(by), None) = (&from, by, pushed_by(&r)) {
        r.links.push(Link { label: PUSHED_BY.into(), to: by.to_string() });
    }
    let origin = from.unwrap_or_else(|| origin.to_string());
    r.links.retain(|l| l.label != SYNCED_FROM && l.label != ACCEPTED);
    json!({
        "origin": origin,
        "id": format!("{project}:{}", r.id),
        "project": project,
        "rid": r.id,
        "session": r.session,
        "kind": r.kind.as_str(),
        "tag": r.tag,
        "turn": r.turn,
        "seq": r.seq,
        "ts": r.ts,
        "importance": r.importance,
        "files": r.files,
        "content": r.text,
        "context": r.ctx,
        "links_json": serde_json::to_string(&r.links).unwrap_or_else(|_| "[]".into()),
    })
}

/// A server's row (in [`REMOTE_COLUMNS`] order) as a record.
fn remote_record(row: &[Json]) -> Option<Record> {
    let s = |i: usize| row.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let n = |i: usize| row.get(i).and_then(|v| v.as_i64()).unwrap_or(0);
    let links: Vec<Link> = serde_json::from_str(&s(11)).unwrap_or_default();
    Some(Record {
        id: Some(s(0)).filter(|x| !x.is_empty())?,
        session: s(1),
        kind: Kind::parse(row.get(2)?.as_str()?)?,
        tag: s(3),
        turn: n(4),
        seq: n(5),
        ts: n(6),
        importance: row.get(7).and_then(|v| v.as_f64()).unwrap_or(0.0),
        files: s(8),
        text: s(9),
        ctx: s(10),
        links,
    })
}

/// Whether `e` says the server has no table `table`.
fn no_table(e: &crate::Error, table: &str) -> bool {
    e.to_string().contains(&format!("no such table: {table}"))
}

/// [`push_once`], creating the project's table first where the server
/// lacks it (a key with the `admin` scope can; the Inspector does it as it
/// makes a project's key).
pub fn push(remote: &Remote, store: &Store, project: &str, st: &mut SyncState) -> Result<usize> {
    match push_once(remote, store, project, st) {
        Err(e) if no_table(&e, &table_for(project)) => {
            remote.create_table(project).map_err(|c| {
                err(format!("{e}; creating it failed too ({c}): an admin makes it, and a key for it, with \"Make a sync key\" on the command center's ChloeCtx page, or with `chloectx sync setup` and an admin's key"))
            })?;
            push_once(remote, store, project, st)
        }
        r => r,
    }
}

/// Sends the records of `store` after `st.pushed_seq`; how many.
fn push_once(remote: &Remote, store: &Store, project: &str, st: &mut SyncState) -> Result<usize> {
    let table = table_for(project);
    let me = origin(&remote.header_dir);
    // Asked once there is something to send: the daemon pushes every 30 s.
    let mut by: Option<Option<String>> = None;
    let mut sent = 0;
    loop {
        let rs = store.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE seq > :s ORDER BY seq ASC LIMIT {BATCH}"), &Params::new().set("s", chloedb::Value::Int(st.pushed_seq)))?;
        let recs: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).collect();
        let Some(top) = rs.rows.iter().filter_map(|r| r.get(5).and_then(|v| v.as_i64())).max() else { break };
        if !recs.is_empty() {
            let by = by.get_or_insert_with(|| remote.key_name()).as_deref();
            let ops: Vec<Json> = recs.iter().map(|r| json!({"op": "upsert", "table": table, "doc": remote_doc(project, &me, by, r)})).collect();
            // Named by the records it holds: a retry of this batch is the
            // same request, and another machine's batch of the same
            // project and sequence numbers (each store numbers its own) is
            // not, which a key of the numbers alone said it was, so the
            // server refused it as a reused key for an hour.
            let ids = recs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(",");
            remote.post("/v1/write", &json!({ "ops": ops, "idempotency_key": format!("chloectx-{:016x}-{:016x}-{top}", fnv(project), fnv(&ids)) }))?;
            sent += recs.len();
        }
        st.pushed_seq = top;
        st.pushed += recs.len() as u64;
        st.last_push = crate::now();
        if rs.rows.len() < BATCH {
            break;
        }
    }
    Ok(sent)
}

/// Sends `recs` again, replacing the server's rows with the same ids (the
/// records `chloectx redact` rewrote); how many.
pub fn push_records(remote: &Remote, project: &str, recs: &[Record]) -> Result<usize> {
    let (table, me, by) = (table_for(project), origin(&remote.header_dir), remote.key_name());
    for batch in recs.chunks(BATCH) {
        let ops: Vec<Json> = batch.iter().map(|r| json!({"op": "upsert", "table": table, "doc": remote_doc(project, &me, by.as_deref(), r)})).collect();
        let ids = batch.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(",");
        remote.post("/v1/write", &json!({ "ops": ops, "idempotency_key": format!("chloectx-redact-{:016x}-{:016x}-{}", fnv(project), fnv(&ids), crate::now()) }))?;
    }
    Ok(recs.len())
}

/// Stores the records of `project` the server holds and `store` lacks:
/// from the project's table, then from the table projects shared before
/// 0.3.4 ([`TABLE`]) where the key may read it. A server without a table
/// holds nothing to pull from it. The records forgotten in the store
/// (its [`Forgotten`], in `store_dir`) are never stored again (audit
/// CTX-33).
pub fn pull(remote: &Remote, store: &Store, project: &str, st: &mut SyncState, store_dir: &Path) -> Result<usize> {
    let cfg = load_config(&remote.header_dir);
    let me = origin(&remote.header_dir);
    let table = table_for(project);
    let mut got = match pull_once(remote, store, project, &table, PROJECT_COLUMNS, &me, &cfg, store_dir) {
        Err(e) if no_table(&e, &table) => 0,
        r => r?,
    };
    // The shared table may be gone, or not for this key (a project's key
    // is refused it): neither is an error of this project's sync.
    got += pull_once(remote, store, project, TABLE, REMOTE_COLUMNS, &me, &cfg, store_dir).unwrap_or(0);
    st.pulled += got as u64;
    st.last_pull = crate::now();
    Ok(got)
}

/// The records of `project` in `table`, read as `cols`, that `store`
/// lacks, stored. Paged by sequence number: each machine numbers its own
/// records, so the server holds several of one number, and a full page
/// may end inside them, so its last number's records are read whole
/// before the next page starts after it.
#[allow(clippy::too_many_arguments)]
fn pull_once(remote: &Remote, store: &Store, project: &str, table: &str, cols: &str, me: &str, cfg: &SyncConfig, store_dir: &Path) -> Result<usize> {
    let mut got = 0;
    let mut after = i64::MIN;
    loop {
        let mut rows = remote.rows(&format!("SELECT {cols} FROM {table} WHERE project = :p AND seq > :s ORDER BY seq ASC LIMIT {BATCH}"), json!({ "p": project, "s": after }))?;
        let full = rows.len() >= BATCH;
        let Some(top) = rows.iter().filter_map(|r| r.get(5).and_then(|v| v.as_i64())).max() else { break };
        if full {
            rows.retain(|r| r.get(5).and_then(|v| v.as_i64()) != Some(top));
            rows.extend(remote.rows(&format!("SELECT {cols} FROM {table} WHERE project = :p AND seq = :s LIMIT 10000"), json!({ "p": project, "s": top }))?);
        }
        got += store_new(store, &rows, project, me, cfg, store_dir)?;
        after = top;
        if !full {
            break;
        }
    }
    Ok(got)
}

/// Stores the server's `rows` that `store` lacks, redacted and embedded
/// here; how many. A row another machine pushed (its `origin`, the
/// thirteenth column, is not `me`; a row of the shared table names none)
/// is linked [`SYNCED_FROM`] that machine, or [`ACCEPTED`] when `cfg`
/// accepted it for `project` ([`SyncConfig::trusts`]). A record forgotten here ([`Forgotten`], in `store_dir`) is
/// passed over: the server, or another machine that pushes it again, may
/// still hold it. The list is read, and the records stored, while a forget
/// cannot add to it, so that one that deletes a record as it is pulled
/// either finds it stored, and deletes it, or keeps it out.
fn store_new(store: &Store, rows: &[Vec<Json>], project: &str, me: &str, cfg: &SyncConfig, store_dir: &Path) -> Result<usize> {
    let _one = FORGOTTEN.lock().unwrap_or_else(|e| e.into_inner());
    let forgotten = load_forgotten(store_dir).ids;
    let mut recs: Vec<Record> = Vec::new();
    for row in rows {
        let Some(mut r) = remote_record(row) else { continue };
        if forgotten.contains(&r.id) {
            continue;
        }
        crate::redact::redact_record(&mut r);
        let from = row.get(12).and_then(|v| v.as_str()).filter(|o| !o.is_empty()).unwrap_or("unknown");
        if from != me {
            let ok = cfg.trusts(project, from);
            r.links.retain(|l| l.label != SYNCED_FROM && l.label != ACCEPTED);
            r.links.push(Link { label: if ok { ACCEPTED } else { SYNCED_FROM }.into(), to: from.to_string() });
        }
        recs.push(r);
    }
    let ids: Vec<String> = recs.iter().map(|r| r.id.clone()).collect();
    let have: std::collections::HashSet<String> = store.get(&ids)?.into_iter().map(|r| r.id).collect();
    let new: Vec<Record> = recs.into_iter().filter(|r| !have.contains(&r.id)).collect();
    if new.is_empty() {
        return Ok(0);
    }
    let texts: Vec<String> = new.iter().map(|r| r.embed_text()).collect();
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let embs = store.embedder.embed(&refs)?;
    store.bulk_insert(&new, embs)?;
    Ok(new.len())
}

/// A machine whose records wait to be accepted (`chloectx sync accept`).
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    /// The machine, as its rows name it ([`origin`]).
    pub origin: String,
    /// The name of the key it pushed them with, when its rows said
    /// ([`PUSHED_BY`]).
    pub key: Option<String>,
    /// How many of its records wait.
    pub records: usize,
    /// A few of its rules, decisions and notes, as they read.
    pub some: Vec<String>,
}

impl Pending {
    /// `ana-laptop (m-44fb96669a15)`, or the id alone.
    pub fn label(&self) -> String {
        machine_label(&self.origin, self.key.as_deref())
    }

    /// Whether `name` names this machine: its id, its key's name, or the
    /// machine that name names ([`machine_name`]).
    pub fn named(&self, name: &str) -> bool {
        let n = name.trim();
        !n.is_empty() && (n == self.origin || self.key.as_deref().is_some_and(|k| k == n || machine_name(k) == n))
    }
}

/// `chloectx sync accept` in project `project` (its [`project_key`]): the
/// store's records pulled from machines not accepted yet, by machine
/// ([`Pending`], named by the key that pushed them where the rows say:
/// WEI-308). With `names` (a machine's id, its key's name, or the machine
/// that key names), or `all` (every machine listed now, and no other),
/// those machines' records are accepted, and the machines added to the
/// project's accepted machines in `sync.json`, so that later pulls of this
/// project take theirs as accepted too. Returns the machines that waited,
/// and the names that matched none.
pub fn accept(home: &Path, store: &Store, project: &str, names: &[String], all: bool) -> Result<(Vec<Pending>, Vec<String>)> {
    // Pulled records keep the server's sequence numbers: paged by them.
    let mut recs: Vec<Record> = Vec::new();
    let mut after = i64::MIN;
    loop {
        let rs = store.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE seq > :s ORDER BY seq ASC LIMIT 5000"), &Params::new().set("s", chloedb::Value::Int(after)))?;
        let mut page: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).collect();
        let Some(top) = page.iter().map(|r| r.seq).max() else { break };
        let full = page.len() >= 5000;
        if full {
            // Several machines' records share a number: the last one's whole.
            page.retain(|r| r.seq != top);
            let rest = store.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE seq = :s LIMIT 100000"), &Params::new().set("s", chloedb::Value::Int(top)))?;
            page.extend(rest.rows.iter().filter_map(|r| crate::store::row_record(r)));
        }
        recs.extend(page.into_iter().filter(pending));
        if !full {
            break;
        }
        after = top;
    }
    let mut by: std::collections::BTreeMap<String, Pending> = Default::default();
    for r in &recs {
        let o = pending_origin(r).unwrap_or("unknown").to_string();
        let e = by.entry(o.clone()).or_insert_with(|| Pending { origin: o, key: None, records: 0, some: Vec::new() });
        e.records += 1;
        if e.key.is_none() {
            e.key = pushed_by(r).map(String::from);
        }
        if e.some.len() < 3 && r.kind.is_knowledge() {
            e.some.push(r.text.chars().take(120).collect());
        }
    }
    let waiting: Vec<Pending> = by.into_values().collect();
    let unknown: Vec<String> = names.iter().filter(|n| *n != "*" && !waiting.iter().any(|p| p.named(n))).cloned().collect();
    // `*` from an older client is every machine listed now, as --all.
    let all = all || names.iter().any(|n| n == "*");
    let chosen: Vec<String> = waiting.iter().filter(|p| all || names.iter().any(|n| p.named(n))).map(|p| p.origin.clone()).collect();
    if !chosen.is_empty() {
        let changed: Vec<Record> = recs
            .into_iter()
            .filter(|r| chosen.iter().any(|o| Some(o.as_str()) == pending_origin(r)))
            .map(|mut r| {
                for l in r.links.iter_mut().filter(|l| l.label == SYNCED_FROM) {
                    l.label = ACCEPTED.into();
                }
                r
            })
            .collect();
        store.insert(&changed)?;
        let mut cfg = load_config(home);
        let list = cfg.accepted.entry(project.to_string()).or_default();
        for o in &chosen {
            if !list.contains(o) {
                list.push(o.clone());
            }
        }
        // Every machine to come, for every project: no longer (WEI-308).
        cfg.trusted.retain(|t| t != "*");
        save_config(home, &cfg)?;
    }
    Ok((waiting, unknown))
}

/// Whether `t` is an Anthropic API key, which a ChloeDB server never takes:
/// a variable set to the wrong key would send it to the sync's server.
fn is_anthropic_key(t: &str) -> bool {
    t.trim_start().starts_with("sk-ant-")
}

pub(crate) fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ANA-3: an origin added changes the name a project syncs as, and so
    /// its table; the marks of the old table sent nothing to the new one.
    #[test]
    fn another_name_starts_its_table_from_the_first_record() {
        let dir = tempfile::tempdir().unwrap();
        let st = SyncState { url: "http://s".into(), key: "home_ana_code_ledger_a521b67e".into(), pushed_seq: 320, pushed: 318, ..Default::default() };
        save_state(dir.path(), &st);
        assert_eq!(load_state(dir.path(), "http://s", "home_ana_code_ledger_a521b67e").pushed_seq, 320);
        let new = load_state(dir.path(), "http://s", "acme/ledger");
        assert_eq!((new.pushed_seq, new.pushed, new.key.as_str()), (0, 0, "acme/ledger"));
        assert_eq!(load_state(dir.path(), "http://other", "home_ana_code_ledger_a521b67e").pushed_seq, 0, "another server too");
        // A state from before the name was kept: taken for the current one.
        std::fs::write(dir.path().join("sync-state.json"), r#"{"url":"http://s","pushed_seq":12,"last_push":0,"last_pull":0,"pushed":12,"pulled":0,"last_error":""}"#).unwrap();
        assert_eq!(load_state(dir.path(), "http://s", "acme/ledger").pushed_seq, 12);
        // Status says when the table holds fewer than were sent.
        assert!(short_on_server(318, 0).unwrap().contains("chloectx sync push --again"));
        assert_eq!(short_on_server(318, 318), None);
        assert_eq!(short_on_server(0, 0), None);
    }

    #[test]
    fn a_project_syncs_once_set_up_there_and_no_other_because_of_it() {
        // LENA-301: the first key set up was the machine's too, and every
        // other project's daemon uploaded its store with it.
        if std::env::var_os("CHLOECTX_SYNC_TOKEN").is_some() || std::env::var_os("CHLOECTX_SYNC_URL").is_some() {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("ctx");
        let (web, shop) = (t.path().join("web"), t.path().join("shop"));
        let url = "https://chloedb.example.com";
        let (web_key, shop_key) = (project_key(&home, &web.to_string_lossy()), project_key(&home, &shop.to_string_lossy()));
        let kept = setup(&home, &web.to_string_lossy(), url, Some("sk_web_1\n"), "", &[], false).unwrap();
        assert_eq!(kept, vec![project_token_path(&home, &web_key)], "the project's key only");
        assert!(!token_path(&home).exists());
        let token = |r: Option<Remote>| r.and_then(|r| r.token).unwrap_or_default();
        assert_eq!(token(remote_for(&home, &web_key)), "sk_web_1");
        assert_eq!(syncs(&home, &web_key), Some("set up in this project"));
        // Another project of the machine does not sync, with any key.
        assert!(remote_for(&home, &shop_key).is_none());
        assert_eq!(syncs(&home, &shop_key), None);
        assert!(remote(&home).is_some_and(|r| !r.has_token()), "a server is set up, and the machine has no key");
        assert_eq!(set_up_projects(&home), vec![web_key.clone()]);
        assert!(scope_line(&home).starts_with(&format!("only {web_key} syncs")), "{}", scope_line(&home));
        // Set up there, it syncs with its own key.
        let kept = setup(&home, &shop.to_string_lossy(), url, Some("sk_shop_2"), "", &[], false).unwrap();
        assert_eq!(kept, vec![project_token_path(&home, &shop_key)]);
        assert_eq!(token(remote_for(&home, &web_key)), "sk_web_1");
        assert_eq!(token(remote_for(&home, &shop_key)), "sk_shop_2");
        assert_eq!(remote_for(&home, &shop_key).unwrap().key_from, "the project's key");
        assert!(remote_for(&home, "acme/other").is_none());
        // A setup without a key (an open server, or --token-env) lists the project too.
        let open = t.path().join("open");
        let open_key = project_key(&home, &open.to_string_lossy());
        assert!(setup(&home, &open.to_string_lossy(), url, None, "", &[], false).unwrap().is_empty());
        assert_eq!(remote_for(&home, &open_key).map(|r| r.key_from), Some("no key".to_string()));
        assert_eq!(set_up_projects(&home), vec![web_key.clone(), shop_key.clone(), open_key.clone()]);
        // --every-project: every project syncs, and one without a key sends the machine's.
        let kept = setup(&home, &shop.to_string_lossy(), url, Some("sk_admin_3"), "", &[], true).unwrap();
        assert_eq!(kept, vec![project_token_path(&home, &shop_key), token_path(&home)]);
        assert_eq!(token(remote_for(&home, "acme/other")), "sk_admin_3");
        assert_eq!(remote_for(&home, "acme/other").unwrap().key_from, "the machine's key (sync.token)");
        assert!(scope_line(&home).starts_with("every project on this machine syncs"));
        // A later setup without the flag keeps it; no key given keeps the keys.
        assert!(setup(&home, &web.to_string_lossy(), url, None, "", &[], false).unwrap().is_empty());
        assert_eq!(token(remote_for(&home, &web_key)), "sk_web_1");
        assert!(remote_for(&home, "acme/other").is_some());
        // Each file is its owner's only.
        use std::os::unix::fs::PermissionsExt;
        for p in [project_token_path(&home, &web_key), token_path(&home)] {
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o077, 0, "{}", p.display());
        }
        // `sync off --every-project`: back to the projects set up, and the machine's key is gone.
        let did = off(&home, &web.to_string_lossy(), true).unwrap();
        assert_eq!(did.len(), 2, "{did:?}");
        assert!(remote_for(&home, "acme/other").is_none() && !token_path(&home).exists());
        assert_eq!(token(remote_for(&home, &shop_key)), "sk_admin_3");
        // `sync off` in a project: it stops, and its key goes.
        let did = off(&home, &shop.to_string_lossy(), false).unwrap();
        assert!(did[0].contains("no longer syncs"), "{did:?}");
        assert!(remote_for(&home, &shop_key).is_none() && !project_token_path(&home, &shop_key).exists());
        assert!(remote_for(&home, &web_key).is_some());
        // A key kept by a setup before projects were listed still counts as set up there.
        let mut cfg = load_config(&home);
        cfg.projects.clear();
        std::fs::write(config_path(&home), serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(token(remote_for(&home, &web_key)), "sk_web_1");
        assert!(remote_for(&home, &open_key).is_none());
        assert_eq!(set_up_projects(&home), vec![format!("the project of table {}", table_for(&web_key))]);
        // ... and the machine's key an earlier setup kept (the first one) is sent for no other.
        std::fs::write(token_path(&home), "sk_web_1").unwrap();
        assert!(remote_for(&home, "acme/other").is_none());
        // A 403 says which key the project needs; another error, nothing.
        let h = key_hint("acme/web", "https://x/v1/write: 403: a write touches table chloectx_mem_acme_web_9, which the key \"k\" may not");
        assert!(h.contains(&table_for("acme/web")) && h.contains("Make a sync key") && h.contains("acme/web"), "{h}");
        assert!(key_hint("acme/web", "https://x/v1/write: curl: (7) Failed to connect").is_empty());
    }

    #[test]
    fn a_remote_names_its_repository() {
        for u in ["https://github.com/chloetech-ai/Strata", "https://github.com/chloetech-ai/strata.git", "git@github.com:chloetech-ai/strata.git", "http://local_proxy@127.0.0.1:1234/git/chloetech-ai/strata"] {
            assert_eq!(repo_of(u).as_deref(), Some("chloetech-ai/strata"), "{u}");
        }
        assert_eq!(repo_of("strata"), None);
    }

    #[test]
    fn each_project_has_a_table_of_its_own() {
        assert_eq!(table_for("chloetech-ai/chloedb"), format!("chloectx_mem_chloetech_ai_chloedb_{:08x}", fnv("chloetech-ai/chloedb") as u32));
        assert_ne!(table_for("acme/web-app"), table_for("acme/web_app"));
        let t = table_for("/home/someone/a folder/with a very long name that goes on and on and on");
        assert!(t.starts_with(PREFIX) && t.len() <= PREFIX.len() + 40 + 9, "{t}");
        assert!(t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "{t}");
        // Not a space's table (`ctx_*`), nor the shared one.
        assert!(!t.starts_with("ctx_") && !t.starts_with(TABLE));
        assert!(create_for("acme/web").contains(&table_for("acme/web")));
    }

    #[test]
    fn plain_http_is_for_this_machine_only() {
        for u in ["https://chloedb.example.com", "HTTPS://x", "http://127.0.0.1:7700", "http://localhost:7700/", "http://[::1]:7700", "http://user@127.0.0.1:1"] {
            assert!(check_url(u).is_ok(), "{u}");
        }
        std::env::remove_var("CHLOECTX_SYNC_ALLOW_HTTP");
        for u in ["http://chloedb.example.com", "http://10.0.0.5:7700", "http://127.0.0.1.example.com", "http://127.0.0.1@evil.example.com/", "ftp://x"] {
            assert!(check_url(u).is_err(), "{u}");
        }
        let r = Remote { url: "http://chloedb.example.com".into(), token: Some("sk_a_b".into()), key_from: String::new(), header_dir: std::env::temp_dir() };
        let e = r.post("/v1/query", &json!({})).unwrap_err().to_string();
        assert!(e.contains("plain HTTP") && !e.contains("sk_a_b"), "{e}");
    }

    #[test]
    fn an_anthropic_key_is_never_sent() {
        let r = Remote { url: "http://127.0.0.1:9".into(), token: Some("sk-ant-api03-xxxx".into()), key_from: String::new(), header_dir: std::env::temp_dir() };
        let e = r.post("/v1/query", &json!({})).unwrap_err().to_string();
        assert!(e.contains("Anthropic API key") && !e.contains("xxxx"), "{e}");
        assert!(!is_anthropic_key("sk_abc_def") && !is_anthropic_key("a-cluster-token"));
    }

    #[test]
    fn records_round_trip_through_the_servers_row() {
        let r = Record {
            id: "abcd1234:7".into(),
            session: "abcd1234-s".into(),
            kind: Kind::Decision,
            tag: "".into(),
            turn: 3,
            seq: 7,
            ts: 1_700_000_000,
            importance: 0.75,
            files: "src/a.rs".into(),
            text: "use ChloeDB".into(),
            ctx: "Turn 3".into(),
            links: vec![Link { label: "about".into(), to: "e:chloedb".into() }],
        };
        let d = remote_doc("/home/me/p", "m-1", None, &r);
        assert_eq!(d["origin"], "m-1");
        assert_eq!(d["id"], "/home/me/p:abcd1234:7");
        let row: Vec<Json> = REMOTE_COLUMNS.split(", ").map(|c| d[c].clone()).collect();
        let back = remote_record(&row).unwrap();
        assert_eq!(back.id, r.id);
        assert_eq!(back.kind, r.kind);
        assert_eq!(back.links, r.links);
        assert_eq!((back.seq, back.text.as_str(), back.ctx.as_str()), (7, "use ChloeDB", "Turn 3"));
    }

    #[test]
    fn what_is_sent_is_redacted_again() {
        // As a store written before redaction knew these shapes holds them.
        let pw = ["correct", "horse", "battery", "42"].join("-");
        let key = format!("sk_{}_{}", "9f8e7d6c5b4a3f2e", "0123456789abcdef".repeat(3));
        let r = Record {
            id: "abcd1234:8".into(),
            session: "abcd1234-s".into(),
            kind: Kind::ToolResult,
            tag: "Bash".into(),
            turn: 3,
            seq: 8,
            ts: 1_700_000_000,
            importance: 0.3,
            files: String::new(),
            text: format!("$ env\nDB_PASSWORD={pw}"),
            ctx: format!("Turn 3 request: use the key {key}"),
            links: Vec::new(),
        };
        let d = remote_doc("p", "m-1", None, &r).to_string();
        assert!(!d.contains(&pw) && !d.contains(&key), "{d}");
        assert!(d.contains("DB_PASSWORD=[REDACTED:password]") && d.contains("use the key [REDACTED:chloedb-key]"), "{d}");
    }
}
