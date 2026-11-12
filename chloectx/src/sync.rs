//! Copies a project's memory to a ChloeDB server or cluster, and back.
//!
//! The store stays a local file, so that hooks and recall never wait on the
//! network. The daemon pushes the records it has not sent yet to the table
//! [`TABLE`] of the server set up with `chloectx sync setup --url URL`
//! every [`PUSH_EVERY`] and as it exits, and when it starts it pulls the
//! records of its project the store lacks, which a store made on another
//! machine (or a cloud container started afresh) wrote, and embeds them
//! here. Records are keyed `<project>:<id>`, so projects share the table;
//! a project is its git repository (`owner/name`, see [`project_key`]), so
//! every checkout of it, under any folder or an old name, shares one memory.
//!
//! What is sent is what the store holds: text from the sessions it
//! recorded, which can include whatever passed through them. It is
//! redacted again as it is sent and as it is pulled ([`crate::redact`]),
//! for stores written before redaction knew a secret's shape. The server
//! should take it over HTTPS and need a key (an API key with the `write`
//! scope is enough once the table exists; creating it needs `admin`).
//!
//! Records retagged or forgotten after they were pushed are not sent
//! again: the server keeps the version it was sent first. Records that
//! `chloectx redact` rewrites are ([`push_records`]).

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

/// The server's table of memory records.
pub const TABLE: &str = "chloectx_memory";
/// How often the daemon pushes what it recorded since.
pub const PUSH_EVERY: Duration = Duration::from_secs(30);
/// Records per request.
const BATCH: usize = 500;
/// A request's deadline.
const TIMEOUT_SECS: u64 = 30;

const CREATE: &str = "CREATE TABLE IF NOT EXISTS chloectx_memory (id KEYWORD PRIMARY KEY, project KEYWORD, rid KEYWORD, session KEYWORD, kind KEYWORD, tag KEYWORD, turn INT, seq INT, ts INT, importance FLOAT, files TEXT, content TEXT, context TEXT, links_json TEXT)";
const REMOTE_COLUMNS: &str = "rid, session, kind, tag, turn, seq, ts, importance, files, content, context, links_json";

/// Where the sync is set up: `sync.json` (the URL) and `sync.token` (the
/// key, readable by its owner only) in ChloeCtx's home.
pub fn config_path(home: &Path) -> PathBuf {
    home.join("sync.json")
}

pub fn token_path(home: &Path) -> PathBuf {
    home.join("sync.token")
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
}

fn load_config(home: &Path) -> SyncConfig {
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

/// The server to sync with: `$CHLOECTX_SYNC_URL`, else `sync.json`; the
/// key: `$CHLOECTX_SYNC_TOKEN`, else the variable `sync.json` names, else
/// `sync.token` (none: the server is open, or a proxy adds it). `None` when
/// no URL is set.
pub fn remote(home: &Path) -> Option<Remote> {
    let cfg = load_config(home);
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let url = env("CHLOECTX_SYNC_URL").or_else(|| Some(cfg.url.clone()).filter(|u| !u.trim().is_empty()))?;
    let token = env("CHLOECTX_SYNC_TOKEN")
        .or_else(|| Some(cfg.token_env.as_str()).filter(|k| !k.is_empty()).and_then(env))
        .or_else(|| std::fs::read_to_string(token_path(home)).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    Some(Remote { url: url.trim().trim_end_matches('/').to_string(), token, header_dir: home.to_path_buf() })
}

/// Writes the sync's settings (and the key, owner-only, when given).
pub fn setup(home: &Path, url: &str, token: Option<&str>, token_env: &str, renames: &[String]) -> Result<()> {
    crate::paths::create_private_dir(home)?;
    let mut map = std::collections::BTreeMap::new();
    for r in renames {
        let (old, new) = r.split_once('=').ok_or_else(|| err(format!("--rename {r}: give OLD=NEW, as owner/old=owner/new")))?;
        map.insert(old.trim().to_lowercase(), new.trim().to_lowercase());
    }
    let cfg = SyncConfig { url: url.trim().to_string(), token_env: token_env.trim().to_string(), renames: map };
    std::fs::write(config_path(home), serde_json::to_string_pretty(&cfg).unwrap_or_default())?;
    if let Some(t) = token {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(token_path(home))?;
        f.write_all(t.trim().as_bytes())?;
    }
    Ok(())
}

/// A ChloeDB server's HTTP API, called with `curl`.
pub struct Remote {
    pub url: String,
    token: Option<String>,
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
        if self.token.as_deref().is_some_and(is_anthropic_key) {
            return Err(err(format!("the sync's key is an Anthropic API key (sk-ant-...), not a ChloeDB token or API key (sk_<id>_<secret>): it was not sent to {}. Set the variable or sync.token to the server's key", self.url)));
        }
        let mut cmd = Command::new("curl");
        cmd.args(["-sS", "-m", &TIMEOUT_SECS.to_string(), "-X", "POST", "-H", "content-type: application/json", "-H", "x-chloedb-client: chloectx-sync", "--data-binary", "@-", "-w", "\n%{http_code}"]);
        let header = match &self.token {
            Some(t) => {
                let p = self.header_dir.join(format!(".sync-header-{}", std::process::id()));
                let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p)?;
                writeln!(f, "Authorization: Bearer {t}")?;
                cmd.arg("-H").arg(format!("@{}", p.display()));
                Some(p)
            }
            None => None,
        };
        cmd.arg(format!("{}{route}", self.url));
        let out = (|| {
            let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| err(format!("curl: {e}")))?;
            child.stdin.take().expect("stdin").write_all(body.to_string().as_bytes())?;
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

    /// Creates the table (once; needs the `admin` scope).
    pub fn create_table(&self) -> Result<()> {
        self.post("/v1/sql", &json!({ "sql": CREATE })).map(|_| ())
    }

    fn rows(&self, sql: &str, params: Json) -> Result<Vec<Vec<Json>>> {
        let j = self.post("/v1/query", &json!({ "sql": sql, "params": params }))?;
        Ok(j.get("rows").and_then(|r| r.as_array()).map(|rs| rs.iter().filter_map(|r| r.as_array().cloned()).collect()).unwrap_or_default())
    }

    /// How many records of `project` the server holds.
    pub fn count(&self, project: &str) -> Result<i64> {
        let rows = self.rows(&format!("SELECT COUNT(*) FROM {TABLE} WHERE project = :p"), json!({ "p": project }))?;
        Ok(rows.first().and_then(|r| r.first()).and_then(|v| v.as_i64()).unwrap_or(0))
    }
}

/// What was pushed, per project store: `sync-state.json` beside the store.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncState {
    /// The server these marks are for: another URL starts again.
    pub url: String,
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

pub fn load_state(store_dir: &Path, url: &str) -> SyncState {
    let s: SyncState = std::fs::read_to_string(state_path(store_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if s.url == url {
        s
    } else {
        SyncState { url: url.to_string(), ..SyncState::default() }
    }
}

pub fn save_state(store_dir: &Path, s: &SyncState) {
    let tmp = state_path(store_dir).with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(s).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, state_path(store_dir));
    }
}

/// A record as the server's row, redacted.
fn remote_doc(project: &str, r: &Record) -> Json {
    let mut r = r.clone();
    crate::redact::redact_record(&mut r);
    json!({
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

/// Whether `e` says the server has no table [`TABLE`].
fn no_table(e: &crate::Error) -> bool {
    let m = e.to_string();
    m.contains(&format!("no such table: {TABLE}"))
}

/// [`push_once`], creating the table first where the server lacks it (a
/// key with the `admin` scope can).
pub fn push(remote: &Remote, store: &Store, project: &str, st: &mut SyncState) -> Result<usize> {
    match push_once(remote, store, project, st) {
        Err(e) if no_table(&e) => {
            remote.create_table().map_err(|c| err(format!("{e}; creating it failed too (it needs a key with the admin scope): {c}")))?;
            push_once(remote, store, project, st)
        }
        r => r,
    }
}

/// Sends the records of `store` after `st.pushed_seq`; how many.
fn push_once(remote: &Remote, store: &Store, project: &str, st: &mut SyncState) -> Result<usize> {
    let mut sent = 0;
    loop {
        let rs = store.query_with(&format!("SELECT {COLUMNS} FROM memory WHERE seq > :s ORDER BY seq ASC LIMIT {BATCH}"), &Params::new().set("s", chloedb::Value::Int(st.pushed_seq)))?;
        let recs: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).collect();
        let Some(top) = rs.rows.iter().filter_map(|r| r.get(5).and_then(|v| v.as_i64())).max() else { break };
        if !recs.is_empty() {
            let ops: Vec<Json> = recs.iter().map(|r| json!({"op": "upsert", "table": TABLE, "doc": remote_doc(project, r)})).collect();
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
    for batch in recs.chunks(BATCH) {
        let ops: Vec<Json> = batch.iter().map(|r| json!({"op": "upsert", "table": TABLE, "doc": remote_doc(project, r)})).collect();
        let ids = batch.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().join(",");
        remote.post("/v1/write", &json!({ "ops": ops, "idempotency_key": format!("chloectx-redact-{:016x}-{:016x}-{}", fnv(project), fnv(&ids), crate::now()) }))?;
    }
    Ok(recs.len())
}

/// [`pull_once`]; a server without the table holds nothing to pull.
pub fn pull(remote: &Remote, store: &Store, project: &str, st: &mut SyncState) -> Result<usize> {
    match pull_once(remote, store, project, st) {
        Err(e) if no_table(&e) => Ok(0),
        r => r,
    }
}

/// Stores the records of `project` the server holds and `store` lacks,
/// embedded here; how many. Paged by sequence number: each machine numbers
/// its own records, so the server holds several of one number, and a full
/// page may end inside them, so its last number's records are read whole
/// before the next page starts after it.
fn pull_once(remote: &Remote, store: &Store, project: &str, st: &mut SyncState) -> Result<usize> {
    let mut got = 0;
    let mut after = i64::MIN;
    loop {
        let mut rows = remote.rows(&format!("SELECT {REMOTE_COLUMNS} FROM {TABLE} WHERE project = :p AND seq > :s ORDER BY seq ASC LIMIT {BATCH}"), json!({ "p": project, "s": after }))?;
        let full = rows.len() >= BATCH;
        let Some(top) = rows.iter().filter_map(|r| r.get(5).and_then(|v| v.as_i64())).max() else { break };
        if full {
            rows.retain(|r| r.get(5).and_then(|v| v.as_i64()) != Some(top));
            rows.extend(remote.rows(&format!("SELECT {REMOTE_COLUMNS} FROM {TABLE} WHERE project = :p AND seq = :s LIMIT 10000"), json!({ "p": project, "s": top }))?);
        }
        got += store_new(store, &rows)?;
        after = top;
        if !full {
            break;
        }
    }
    st.pulled += got as u64;
    st.last_pull = crate::now();
    Ok(got)
}

/// Stores the server's `rows` that `store` lacks, redacted and embedded
/// here; how many.
fn store_new(store: &Store, rows: &[Vec<Json>]) -> Result<usize> {
    let mut recs: Vec<Record> = rows.iter().filter_map(|r| remote_record(r)).collect();
    for r in &mut recs {
        crate::redact::redact_record(r);
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

/// Whether `t` is an Anthropic API key, which a ChloeDB server never takes:
/// a variable set to the wrong key would send it to the sync's server.
fn is_anthropic_key(t: &str) -> bool {
    t.trim_start().starts_with("sk-ant-")
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_names_its_repository() {
        for u in ["https://github.com/chloetech-ai/Strata", "https://github.com/chloetech-ai/strata.git", "git@github.com:chloetech-ai/strata.git", "http://local_proxy@127.0.0.1:1234/git/chloetech-ai/strata"] {
            assert_eq!(repo_of(u).as_deref(), Some("chloetech-ai/strata"), "{u}");
        }
        assert_eq!(repo_of("strata"), None);
    }

    #[test]
    fn an_anthropic_key_is_never_sent() {
        let r = Remote { url: "http://127.0.0.1:9".into(), token: Some("sk-ant-api03-xxxx".into()), header_dir: std::env::temp_dir() };
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
        let d = remote_doc("/home/me/p", &r);
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
        let d = remote_doc("p", &r).to_string();
        assert!(!d.contains(&pw) && !d.contains(&key), "{d}");
        assert!(d.contains("DB_PASSWORD=[REDACTED:password]") && d.contains("use the key [REDACTED:chloedb-key]"), "{d}");
    }
}
