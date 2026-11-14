//! Repositories Workbench knows without an agent (sixth build): a person
//! points Workbench at a checkout to read it (search, Map, Trace, Library,
//! Health) before, or instead of, starting an agent on it. Each one opened
//! is a row of `repos`; the list a picker shows is those rows and the
//! repositories agents worked in, the most recent first. Opening one
//! indexes it for code search in the background.
//!
//! `GET /api/repos`, `POST /api/repos {path}`, `DELETE /api/repos?path=`,
//! `GET /api/repos/dirs?q=` (path completion: folders under what was
//! typed, git checkouts first).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json as AxJson, Router};
use chloedb::query::Params;
use chloedb::Value;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::db::{get_ms, get_str, iso, now_ms, ts};
use crate::server::{blocking, parse_body, ApiResult, App};
use crate::supervisor::{ApiError, Supervisor};

pub const TABLE: &str = "CREATE TABLE IF NOT EXISTS repos (id KEYWORD PRIMARY KEY, name KEYWORD, added TIMESTAMP, opened TIMESTAMP)";

/// Folders listed by one completion, and entries read to find them.
const MAX_DIRS: usize = 40;
const MAX_SCAN: usize = 5000;

pub fn routes() -> Router<App> {
    Router::new().route("/api/repos", get(list_route).post(add_route).delete(forget_route)).route("/api/repos/dirs", get(dirs_route))
}

/// `~` and `~/x` as the home folder.
pub fn expand_home(p: &str) -> PathBuf {
    if p == "~" || p.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(p.trim_start_matches('~').trim_start_matches('/'));
        }
    }
    PathBuf::from(p)
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// The main checkout of what a person typed: an absolute path (or `~/…`)
/// of a git checkout, any worktree or folder of it.
pub fn resolve(path: &str) -> Result<PathBuf, ApiError> {
    let t = path.trim();
    if t.is_empty() {
        return Err(ApiError::bad("path is empty: the folder of a git checkout, such as /home/you/src/shop"));
    }
    let p = expand_home(t);
    if !p.is_absolute() {
        return Err(ApiError::bad(format!("{t} is not an absolute path: start it with / or ~/")));
    }
    if !p.is_dir() {
        return Err(ApiError::bad(format!("there is no folder {}", p.display())));
    }
    crate::git::checkout(&p).map(|c| c.main).map_err(|_| ApiError::bad(format!("{} is not in a git checkout: Workbench reads a repository's tracked files and history", p.display())))
}

/// Opens a repository: its row (added, or opened again), and its index
/// started.
pub fn add(sup: &std::sync::Arc<Supervisor>, path: &str) -> Result<Json, ApiError> {
    let main = resolve(path)?;
    let id = main.to_string_lossy().to_string();
    let now = now_ms();
    let had = sup.db().query("SELECT added FROM repos WHERE id = :id LIMIT 1", &Params::new().set("id", Value::Str(id.clone()))).map_err(ApiError::internal)?;
    let added = had.first().and_then(|r| get_ms(&r[0])).unwrap_or(now);
    let p = Params::new().set("id", Value::Str(id.clone())).set("name", Value::Str(name_of(&main))).set("added", ts(added)).set("opened", ts(now));
    sup.db().query("INSERT OR REPLACE INTO repos (id, name, added, opened) VALUES (:id, :name, :added, :opened)", &p).map_err(ApiError::internal)?;
    let index = crate::search::open_repo(sup, &id);
    let mut r = one(&main, None, Some(now), &HashMap::new());
    r["index"] = index;
    Ok(json!({ "repo": r, "created": had.is_empty() }))
}

/// Forgets a repository a person opened (its agents' still list it).
pub fn forget(sup: &Supervisor, path: &str) -> Result<Json, ApiError> {
    let p = expand_home(path.trim());
    let id = crate::git::checkout(&p).map(|c| c.main).unwrap_or(p).to_string_lossy().to_string();
    sup.db().query("DELETE FROM repos WHERE id = :id", &Params::new().set("id", Value::Str(id.clone()))).map_err(ApiError::internal)?;
    Ok(json!({ "ok": true, "repo": id }))
}

/// One repository as the API answers it.
fn one(main: &Path, added: Option<i64>, opened: Option<i64>, agents: &HashMap<String, (usize, usize, i64)>) -> Json {
    let id = main.to_string_lossy().to_string();
    let (running, total, last_agent) = agents.get(&id).copied().unwrap_or((0, 0, 0));
    let last = opened.unwrap_or(0).max(last_agent);
    json!({
        "path": id,
        "name": name_of(main),
        "exists": main.is_dir(),
        "branch": if main.is_dir() { crate::git::current_branch(main) } else { None },
        "opened": opened.map(iso),
        "added": added.map(iso),
        "agents_running": running,
        "agents": total,
        "last_ms": last,
    })
}

/// The repositories a person opened and the ones agents worked in, the
/// most recently used first.
pub fn list(sup: &Supervisor) -> Vec<Json> {
    let mut agents: HashMap<String, (usize, usize, i64)> = HashMap::new();
    for a in sup.agent_rows() {
        let e = agents.entry(a.repo.clone()).or_default();
        e.0 += usize::from(sup.is_running(&a.id));
        e.1 += 1;
        e.2 = e.2.max(a.created).max(a.ended.unwrap_or(0));
    }
    let mut rows: HashMap<String, (Option<i64>, Option<i64>)> = HashMap::new();
    for r in sup.db().query("SELECT id, added, opened FROM repos LIMIT 10000", &Params::new()).unwrap_or_default() {
        if let Some(id) = get_str(&r[0]) {
            rows.insert(id, (get_ms(&r[1]), get_ms(&r[2])));
        }
    }
    let mut out: Vec<Json> = Vec::new();
    for id in rows.keys().chain(agents.keys().filter(|k| !rows.contains_key(*k))) {
        if id.is_empty() {
            continue;
        }
        let (added, opened) = rows.get(id).copied().unwrap_or((None, None));
        out.push(one(Path::new(id), added, opened, &agents));
    }
    out.sort_by(|a, b| b["last_ms"].as_i64().cmp(&a["last_ms"].as_i64()).then(a["path"].as_str().cmp(&b["path"].as_str())));
    out
}

/// Path completion: the folders under what was typed (`/home/me/sr` lists
/// `/home/me/src…`; `/home/me/src/` lists its folders), git checkouts
/// first, then by name; hidden folders only when the typed part starts
/// with a dot. An empty `q` lists the home folder.
pub fn dirs(q: &str) -> Json {
    let q = q.trim();
    let typed = if q.is_empty() { "~/".to_string() } else { q.to_string() };
    let full = expand_home(&typed);
    if !full.is_absolute() {
        return json!({ "q": q, "dirs": [], "missing": ["a repository is an absolute path: start with / or ~/"] });
    }
    let s = full.to_string_lossy().to_string();
    let (parent, prefix) = if s.ends_with('/') { (full.clone(), String::new()) } else { (full.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/")), name_of(&full)) };
    let Ok(rd) = std::fs::read_dir(&parent) else {
        return json!({ "q": q, "parent": parent, "dirs": [], "missing": [format!("cannot read {}", parent.display())] });
    };
    let lower = prefix.to_lowercase();
    let mut found: Vec<(bool, String, PathBuf)> = Vec::new();
    for e in rd.flatten().take(MAX_SCAN) {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && !prefix.starts_with('.') {
            continue;
        }
        if !name.to_lowercase().starts_with(&lower) {
            continue;
        }
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let git = p.join(".git").exists();
        found.push((git, name, p));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let more = found.len() > MAX_DIRS;
    found.truncate(MAX_DIRS);
    let here_git = parent.join(".git").exists();
    json!({
        "q": q,
        "parent": parent,
        "parent_git": here_git,
        "dirs": found.into_iter().map(|(git, name, p)| json!({ "path": p, "name": name, "git": git })).collect::<Vec<_>>(),
        "more": more,
        "missing": [],
    })
}

async fn list_route(State(app): State<App>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || Ok(json!({ "repos": list(&sup) }))).await?;
    Ok(AxJson(v).into_response())
}

#[derive(Deserialize)]
struct AddB {
    path: Option<String>,
    repo: Option<String>,
}

async fn add_route(State(app): State<App>, body: Bytes) -> ApiResult {
    let b: AddB = parse_body(&body)?;
    let sup = app.sup.clone();
    let path = b.path.or(b.repo).unwrap_or_default();
    let v = blocking(move || add(&sup, &path)).await?;
    let status = if v["created"] == true { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, AxJson(v)).into_response())
}

#[derive(Deserialize)]
struct PathQ {
    path: Option<String>,
    q: Option<String>,
}

async fn forget_route(State(app): State<App>, Query(q): Query<PathQ>) -> ApiResult {
    let sup = app.sup.clone();
    let path = q.path.unwrap_or_default();
    Ok(AxJson(blocking(move || forget(&sup, &path)).await?).into_response())
}

async fn dirs_route(Query(q): Query<PathQ>) -> ApiResult {
    let typed = q.q.unwrap_or_default();
    Ok(AxJson(blocking(move || Ok(dirs(&typed))).await?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_folders_with_git_checkouts_first() {
        let d = tempfile::tempdir().unwrap();
        for n in ["shop", "shelf", "Scripts", ".secret", "zed"] {
            std::fs::create_dir(d.path().join(n)).unwrap();
        }
        std::fs::create_dir(d.path().join("shelf/.git")).unwrap();
        std::fs::write(d.path().join("show.txt"), "not a folder").unwrap();
        let base = d.path().to_string_lossy().to_string();
        let v = dirs(&format!("{base}/s"));
        let names: Vec<&str> = v["dirs"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["shelf", "Scripts", "shop"], "{v}");
        assert_eq!(v["dirs"][0]["git"], true);
        let v = dirs(&format!("{base}/"));
        assert_eq!(v["dirs"].as_array().unwrap().len(), 4, "hidden ones only when asked: {v}");
        let v = dirs(&format!("{base}/.s"));
        assert_eq!(v["dirs"][0]["name"], ".secret");
        assert!(dirs("relative/path")["missing"][0].as_str().unwrap().contains("absolute"));
        assert!(resolve("relative").unwrap_err().message.contains("absolute"));
        assert!(resolve(&format!("{base}/zed")).unwrap_err().message.contains("not in a git checkout"));
        assert!(resolve(&format!("{base}/nope")).unwrap_err().message.contains("no folder"));
    }
}
