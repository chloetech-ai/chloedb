//! Projects from GitHub (SPEC.md, "Open a project from GitHub"): a person
//! types `owner/repo`, its GitHub address, or a git https address, and
//! Workbench clones it into its data dir (`repos/<owner>/<name>`, blobless:
//! the whole history, the files' contents on demand), forks it when the
//! signed-in `gh` user cannot push to it, reads what kind of project it is
//! (its language, test command, run command, CI) and opens it as "Open a
//! folder" does. The detected test command and the default branch become
//! the project's Converge settings, and a repository you cannot push to is
//! `untrusted`: fix agents ask before anything their list does not allow.
//!
//! `POST /api/repos/import {source, async?, fork?}` (`fork: false`: never fork) answers
//! `{path, project, forked, untrusted, upstream, fork, default_branch, reused, detected: {language, test_cmd, run_cmd, ci}}`;
//! with `async: true` it answers `{id}` at once and
//! `GET /api/repos/import/:id` is the job: `{id, state, step, steps, result, error}`.
//! `gh` is `CHLOEDB_WORKBENCH_CONVERGE_GH` when set, as for Converge.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path as AxPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::db::now_ms;
use crate::server::{blocking, parse_body, ApiResult, App};
use crate::supervisor::{one_line, ApiError, Supervisor};

pub fn routes() -> Router<App> {
    Router::new().route("/api/repos/import", post(import_route)).route("/api/repos/import/:id", get(job_route))
}

// ------------------------------------------------------------------ the source

/// What a person typed, checked: a GitHub repository, or another host's
/// repository over https.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    GitHub { owner: String, name: String },
    Git { url: String, host: String, owner: String, name: String },
}

impl Source {
    /// Its folder under the data dir's `repos/`.
    fn rel(&self) -> PathBuf {
        match self {
            Source::GitHub { owner, name } => PathBuf::from(owner).join(name),
            Source::Git { host, owner, name, .. } => PathBuf::from(host).join(owner).join(name),
        }
    }
    fn slug(&self) -> Option<String> {
        match self {
            Source::GitHub { owner, name } => Some(format!("{owner}/{name}")),
            Source::Git { .. } => None,
        }
    }
}

/// A GitHub owner: letters, digits and `-`, not first, at most 39.
fn owner_ok(o: &str) -> bool {
    (1..=39).contains(&o.len()) && !o.starts_with('-') && o.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A repository's name: letters, digits, `.`, `_` and `-`, not `.`/`..`, not starting with `-` or `.`.
fn name_ok(n: &str) -> bool {
    (1..=100).contains(&n.len()) && !n.starts_with('-') && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn host_ok(h: &str) -> bool {
    (1..=253).contains(&h.len()) && h.contains('.') && !h.starts_with(['-', '.']) && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

/// Reads `owner/repo`, `https://github.com/owner/repo(.git)`,
/// `git@github.com:owner/repo.git` or `https://host/owner/repo(.git)`;
/// anything else (a local path, `file://`, `ssh://`, `ext::`, options,
/// spaces) is refused.
pub fn parse_source(raw: &str) -> Result<Source, String> {
    let t = raw.trim();
    let say = |why: &str| Err(format!("{why}: type owner/repo (such as pallets/itsdangerous) or https://github.com/owner/repo"));
    if t.is_empty() {
        return say("nothing to import");
    }
    if t.len() > 300 || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return say("that is not a repository");
    }
    let strip = |p: &str| p.trim_end_matches('/').trim_end_matches(".git").to_string();
    let (host, path) = if let Some(rest) = t.strip_prefix("git@github.com:") {
        ("github.com".to_string(), strip(rest))
    } else if let Some(rest) = t.strip_prefix("https://") {
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        let Some((host, path)) = rest.split_once('/') else { return say("that address names no repository") };
        if host.contains('@') || host.contains(':') {
            return say("an address with a user or a port is not taken");
        }
        let host = host.to_ascii_lowercase();
        let host = host.strip_prefix("www.").map(str::to_string).unwrap_or(host);
        (host, strip(path))
    } else if t.contains("://") || t.starts_with('/') || t.starts_with('~') || t.starts_with('.') || t.starts_with('-') || t.contains(':') {
        return say("only GitHub repositories and https addresses are taken, not local folders or other schemes");
    } else if let Some(rest) = t.strip_prefix("github.com/") {
        ("github.com".to_string(), strip(rest))
    } else {
        ("github.com".to_string(), strip(t))
    };
    let parts: Vec<&str> = path.split('/').collect();
    if host == "github.com" {
        // A repository's page (`/tree/main/src`) is the repository.
        if parts.len() < 2 || !owner_ok(parts[0]) || !name_ok(parts[1]) || (parts.len() > 2 && !matches!(parts[2], "tree" | "blob" | "pulls" | "issues" | "")) {
            return say("that is not a GitHub repository");
        }
        return Ok(Source::GitHub { owner: parts[0].to_string(), name: parts[1].to_string() });
    }
    if !host_ok(&host) || parts.len() < 2 || parts.iter().any(|p| !name_ok(p)) {
        return say("that is not a git address");
    }
    let (owner, name) = (parts[parts.len() - 2].to_string(), parts[parts.len() - 1].to_string());
    Ok(Source::Git { url: format!("https://{host}/{}.git", parts.join("/")), host, owner, name })
}

// ------------------------------------------------------------------ jobs

fn jobs() -> &'static Mutex<HashMap<String, Json>> {
    static J: std::sync::OnceLock<Mutex<HashMap<String, Json>>> = std::sync::OnceLock::new();
    J.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Folders being cloned now: one import of a repository at a time.
fn busy() -> &'static Mutex<HashSet<PathBuf>> {
    static B: std::sync::OnceLock<Mutex<HashSet<PathBuf>>> = std::sync::OnceLock::new();
    B.get_or_init(|| Mutex::new(HashSet::new()))
}

fn step(job: &str, name: &str, text: &str) {
    let mut j = jobs().lock().unwrap();
    if let Some(v) = j.get_mut(job) {
        v["step"] = json!(name);
        if let Some(a) = v["steps"].as_array_mut() {
            a.push(json!({ "step": name, "text": text, "ts": now_ms() }));
        }
    }
}

fn finish(job: &str, r: &Result<Json, ApiError>) {
    let mut j = jobs().lock().unwrap();
    if let Some(v) = j.get_mut(job) {
        match r {
            Ok(x) => {
                v["state"] = json!("done");
                v["result"] = x.clone();
            }
            Err(e) => {
                v["state"] = json!("failed");
                v["error"] = json!(e.message);
            }
        }
    }
    // Keep the last 50 jobs.
    if j.len() > 50 {
        let mut ids: Vec<(i64, String)> = j.iter().map(|(k, v)| (v["started"].as_i64().unwrap_or(0), k.clone())).collect();
        ids.sort();
        for (_, k) in ids.into_iter().take(j.len() - 50) {
            j.remove(&k);
        }
    }
}

// ------------------------------------------------------------------ programs

fn gh_run(cwd: &Path, args: &[&str], secs: u64) -> Result<String, String> {
    let a: Vec<String> = args.iter().map(|x| x.to_string()).collect();
    match crate::converge::run_cmd(&crate::converge::gh(), &a, cwd, None, &[("GH_PROMPT_DISABLED", "1".into())], Duration::from_secs(secs))? {
        (0, out, _) => Ok(out),
        (code, out, err) => Err(format!("gh {} failed (exit {code}): {}", args.first().copied().unwrap_or(""), one_line(&format!("{err}{out}"), 300))),
    }
}

fn git_run(cwd: &Path, args: &[&str], secs: u64) -> Result<String, String> {
    let a: Vec<String> = args.iter().map(|x| x.to_string()).collect();
    match crate::converge::run_cmd("git", &a, cwd, None, &[], Duration::from_secs(secs))? {
        (0, out, _) => Ok(out.trim().to_string()),
        (code, _, err) => Err(format!("git {} failed (exit {code}): {}", args.first().copied().unwrap_or(""), one_line(err.trim(), 300))),
    }
}

fn has_remote(dir: &Path, name: &str) -> bool {
    git_run(dir, &["remote"], 30).map(|o| o.lines().any(|l| l.trim() == name)).unwrap_or(false)
}

/// The repository's default branch: GitHub's answer, else the remote's HEAD, else the checkout's.
fn default_branch(dir: &Path, gh: Option<&str>) -> Option<String> {
    if let Some(b) = gh.filter(|b| !b.is_empty()) {
        return Some(b.to_string());
    }
    for remote in ["upstream", "origin"] {
        if let Ok(r) = git_run(dir, &["symbolic-ref", "--short", &format!("refs/remotes/{remote}/HEAD")], 30) {
            if let Some((_, b)) = r.split_once('/') {
                return Some(b.to_string());
            }
        }
    }
    crate::git::current_branch(dir)
}

// ------------------------------------------------------------------ detection

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

fn read(dir: &Path, f: &str) -> String {
    std::fs::read_to_string(dir.join(f)).unwrap_or_default()
}

/// What kind of project a checkout is: its language, its test command, a
/// command that runs it when one is plain, and whether it has GitHub CI.
pub fn detect(dir: &Path) -> Json {
    let has = |f: &str| dir.join(f).exists();
    let pkg: Json = serde_json::from_str(&read(dir, "package.json")).unwrap_or(Json::Null);
    let pyproject = read(dir, "pyproject.toml");
    let python = has("pyproject.toml") || has("setup.py") || has("setup.cfg") || has("requirements.txt");
    let language = if has("Cargo.toml") {
        "Rust"
    } else if has("go.mod") {
        "Go"
    } else if pkg.is_object() {
        if has("tsconfig.json") { "TypeScript" } else { "JavaScript" }
    } else if python {
        "Python"
    } else if has("pom.xml") || has("build.gradle") || has("build.gradle.kts") {
        "Java"
    } else if has("Gemfile") {
        "Ruby"
    } else {
        ""
    };
    let npm_test = pkg["scripts"]["test"].as_str().is_some_and(|t| !t.trim().is_empty() && !t.contains("no test specified"));
    let pytest_marks = ["pytest.ini", "conftest.py", "tests/conftest.py", "tox.ini"].iter().any(|f| has(f))
        || pyproject.contains("pytest")
        || read(dir, "setup.cfg").contains("pytest")
        || ["requirements-dev.txt", "requirements/tests.txt", "requirements/dev.txt", "requirements-test.txt"].iter().any(|f| read(dir, f).contains("pytest"))
        || std::fs::read_dir(dir.join("tests")).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with("test_"))).unwrap_or(false);
    let makefile = read(dir, "Makefile");
    let make_test = makefile.lines().any(|l| l.starts_with("test:") || l.starts_with("test :"));
    let test_cmd = if has("Cargo.toml") {
        Some("cargo test".to_string())
    } else if npm_test {
        Some("npm test".to_string())
    } else if python && pytest_marks {
        Some(if on_path("uv") { "uv run --with pytest python -m pytest -q".to_string() } else { "python -m pytest -q".to_string() })
    } else if has("go.mod") {
        Some("go test ./...".to_string())
    } else if make_test {
        Some("make test".to_string())
    } else {
        None
    };
    let run_cmd = run_command(dir, &pkg, &pyproject);
    let ci = std::fs::read_dir(dir.join(".github/workflows")).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".yml") || e.file_name().to_string_lossy().ends_with(".yaml"))).unwrap_or(false);
    json!({ "language": language, "test_cmd": test_cmd, "run_cmd": run_cmd, "ci": ci })
}

fn run_command(dir: &Path, pkg: &Json, pyproject: &str) -> Option<String> {
    if pkg["scripts"]["start"].is_string() {
        return Some("npm start".into());
    }
    if pkg["scripts"]["dev"].is_string() {
        return Some("npm run dev".into());
    }
    // The first console script of [project.scripts].
    let mut inside = false;
    for l in pyproject.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            inside = l == "[project.scripts]";
            continue;
        }
        if inside {
            if let Some((k, _)) = l.split_once('=') {
                let k = k.trim().trim_matches('"');
                if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
                    return Some(if on_path("uv") { format!("uv run {k}") } else { k.to_string() });
                }
            }
        }
    }
    if dir.join("Cargo.toml").exists() && dir.join("src/main.rs").exists() {
        return Some("cargo run".into());
    }
    if dir.join("go.mod").exists() && dir.join("main.go").exists() {
        return Some("go run .".into());
    }
    // A README's code block that starts the project.
    let readme = ["README.md", "README.rst", "README", "readme.md"].iter().map(|f| read(dir, f)).find(|r| !r.is_empty()).unwrap_or_default();
    let starts = ["npm start", "npm run dev", "cargo run", "python -m ", "python3 -m ", "uv run ", "flask run", "docker compose up", "make run", "go run "];
    let mut code = false;
    for l in readme.lines() {
        let t = l.trim();
        if t.starts_with("```") || t.starts_with("::") {
            code = !code || t.starts_with("::");
            continue;
        }
        let t = t.trim_start_matches("$ ");
        if (code || l.starts_with("    ")) && starts.iter().any(|s| t.starts_with(s)) && t.len() <= 120 && !t.contains("pip install") && !t.contains("pytest") {
            return Some(t.to_string());
        }
    }
    None
}

// ------------------------------------------------------------------ the import

/// Clones (or fetches), forks when you cannot push, detects and opens it.
pub fn import(sup: &Arc<Supervisor>, src: &Source, job: &str, may_fork: bool) -> Result<Json, ApiError> {
    let root = sup.cfg.data_dir.join("repos");
    let dest = root.join(src.rel());
    if !busy().lock().unwrap().insert(dest.clone()) {
        return Err(ApiError::conflict("that repository is being imported already: wait for that import"));
    }
    let r = import_in(sup, src, job, &dest, may_fork);
    busy().lock().unwrap().remove(&dest);
    r
}

fn import_in(sup: &Arc<Supervisor>, src: &Source, job: &str, dest: &Path, may_fork: bool) -> Result<Json, ApiError> {
    let parent = dest.parent().unwrap_or(dest).to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|e| ApiError::internal(format!("cannot create {}: {e}", parent.display())))?;
    let reused = dest.join(".git").exists();
    let label = src.slug().unwrap_or_else(|| match src {
        Source::Git { url, .. } => url.clone(),
        _ => String::new(),
    });
    let fail = |e: String| ApiError::new(502, "import_failed", e);
    if reused {
        step(job, "fetching", &format!("Already cloned: fetching {label}"));
        git_run(dest, &["fetch", "--all", "--prune", "--quiet"], 1800).map_err(fail)?;
        // The default branch follows its remote when it can without a merge.
        let _ = git_run(dest, &["merge", "--ff-only", "--quiet", "@{u}"], 120);
    } else {
        step(job, "cloning", &format!("Cloning {label} (blobless: the history, and the files as they are read)"));
        let d = dest.to_string_lossy().to_string();
        let cloned = match src {
            Source::GitHub { owner, name } => {
                let slug = format!("{owner}/{name}");
                match gh_run(&parent, &["repo", "clone", &slug, &d, "--", "--filter=blob:none", "--quiet"], 3600) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(dest);
                        let url = format!("https://github.com/{slug}.git");
                        git_run(&parent, &["clone", "--filter=blob:none", "--quiet", "--", &url, &d], 3600).map(|_| ()).map_err(|g| format!("{e}; then {g}"))
                    }
                }
            }
            Source::Git { url, .. } => git_run(&parent, &["clone", "--filter=blob:none", "--quiet", "--", url, &d], 3600).map(|_| ()),
        };
        if let Err(e) = cloned {
            let _ = std::fs::remove_dir_all(dest);
            return Err(fail(format!("could not clone {label}: {e}")));
        }
    }
    let dest = dest.canonicalize().map_err(|e| ApiError::internal(e.to_string()))?;

    // Who may push, and the default branch.
    let mut notes: Vec<String> = Vec::new();
    let (push, gh_default) = match src.slug() {
        Some(slug) => {
            step(job, "checking", &format!("Checking whether you can push to {slug}"));
            match gh_run(&dest, &["api", &format!("repos/{slug}")], 120).ok().and_then(|o| serde_json::from_str::<Json>(o.trim()).ok()) {
                Some(v) => (v["permissions"]["push"].as_bool(), v["default_branch"].as_str().map(str::to_string)),
                None => {
                    notes.push("gh could not say whether you can push (is it signed in? `gh auth status`): treated as code you do not own".into());
                    (None, None)
                }
            }
        }
        None => (None, None),
    };
    let untrusted = push != Some(true);
    let mut forked = has_remote(&dest, "upstream") && src.slug().is_some();
    if push == Some(false) && !forked && !may_fork {
        notes.push("not forked (fork: false): fixes can be merged locally, or import it again to fork it".into());
    }
    if push == Some(false) && !forked && may_fork {
        step(job, "forking", "You cannot push there: forking it, so pull requests come from your fork");
        match gh_run(&dest, &["repo", "fork", "--remote", "--remote-name", "origin"], 300) {
            Ok(_) if has_remote(&dest, "upstream") => forked = true,
            Ok(_) => notes.push("gh forked it but added no upstream remote: pull requests go to origin".into()),
            Err(e) => notes.push(format!("could not fork it ({e}): fixes can be merged locally, or open pull requests yourself")),
        }
        if forked {
            let _ = git_run(&dest, &["fetch", "--quiet", "origin"], 600);
        }
    }
    let fork = if forked { crate::converge::gh_slug(&dest) } else { None };
    let upstream = if forked { src.slug() } else { None };
    let default = default_branch(&dest, gh_default.as_deref());

    step(job, "detecting", "Reading the project: language, test and run commands, CI");
    let detected = detect(&dest);

    // Opened as "Open a folder" opens one; Workbench answers Claude Code's
    // trust prompt in its own checkout only for code you can push to.
    let path = dest.to_string_lossy().to_string();
    let opened = crate::repos::add_trusting(sup, &path, Some(!untrusted))?;
    let repo = opened["repo"]["path"].as_str().unwrap_or(&path).to_string();
    let mut st = crate::converge::settings(sup, &repo);
    if st.test_cmd.as_deref().is_none_or(|c| c.trim().is_empty()) {
        st.test_cmd = detected["test_cmd"].as_str().map(str::to_string);
    }
    if st.base.as_deref().is_none_or(str::is_empty) {
        st.base = default.clone();
    }
    // The test command's own runner must be allowed, or the fix agent stops
    // at its first test run (uv, go, make are not in the defaults).
    if let Some(tool) = st.test_cmd.as_deref().and_then(|c| c.split_whitespace().next()).map(str::to_string) {
        let rule = format!("Bash({tool}:*)");
        if !st.dev_allow.iter().any(|r| r == &rule) && !st.dev_deny.iter().any(|r| r == &rule) {
            st.dev_allow.push(rule);
        }
    }
    st.untrusted = untrusted;
    if untrusted && matches!(st.dev_permission_mode.as_deref(), None | Some("auto") | Some("bypassPermissions")) {
        st.dev_permission_mode = Some("default".into());
    }
    if upstream.is_some() {
        st.upstream = upstream.clone();
    }
    crate::converge::put_settings(sup, &repo, &st)?;
    step(job, "done", "Opened");
    let warning = untrusted.then(|| "You do not own this code. Personas and fix agents will run it on this machine: fix agents ask before any command their list does not allow, and nothing is isolated in a container yet.".to_string());
    Ok(json!({
        "path": repo,
        "project": opened["repo"]["project"],
        "source": label,
        "forked": forked,
        "fork": fork,
        "upstream": upstream,
        "untrusted": untrusted,
        "can_push": push,
        "default_branch": default,
        "reused": reused,
        "detected": detected,
        "settings": { "test_cmd": st.test_cmd, "base": st.base, "dev_permission_mode": st.dev_permission_mode },
        "warning": warning,
        "notes": notes,
        "repo": opened["repo"],
    }))
}

#[derive(Deserialize)]
struct ImportB {
    source: Option<String>,
    #[serde(rename = "async")]
    run_async: Option<bool>,
    /// Fork it when you cannot push (default true).
    fork: Option<bool>,
}

async fn import_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    // It clones, and may fork on your GitHub account: your token only.
    crate::converge::user_only(&app, &headers)?;
    let b: ImportB = parse_body(&body)?;
    let src = parse_source(b.source.as_deref().unwrap_or("")).map_err(ApiError::bad)?;
    let id = crate::levels::new_id("imp");
    jobs().lock().unwrap().insert(id.clone(), json!({ "id": id, "source": b.source, "state": "running", "step": "starting", "steps": [], "started": now_ms(), "result": null, "error": null }));
    let may_fork = b.fork.unwrap_or(true);
    let sup = app.sup.clone();
    if b.run_async.unwrap_or(false) {
        let id2 = id.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| import(&sup, &src, &id2, may_fork))).unwrap_or_else(|_| Err(ApiError::internal("the import stopped unexpectedly")));
            finish(&id2, &r);
        });
        return Ok((StatusCode::ACCEPTED, AxJson(json!({ "id": id, "state": "running" }))).into_response());
    }
    let id2 = id.clone();
    let r = blocking(move || {
        let r = import(&sup, &src, &id2, may_fork);
        finish(&id2, &r);
        r
    })
    .await;
    let mut v = r?;
    v["id"] = json!(id);
    Ok((StatusCode::CREATED, AxJson(v)).into_response())
}

async fn job_route(AxPath(id): AxPath<String>) -> ApiResult {
    match jobs().lock().unwrap().get(&id) {
        Some(v) => Ok(AxJson(v.clone()).into_response()),
        None => Err(ApiError::not_found(format!("no import {id}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh(o: &str, n: &str) -> Source {
        Source::GitHub { owner: o.into(), name: n.into() }
    }

    #[test]
    fn takes_owner_repo_and_github_addresses() {
        for t in [
            "pallets/itsdangerous",
            " pallets/itsdangerous ",
            "https://github.com/pallets/itsdangerous",
            "https://github.com/pallets/itsdangerous.git",
            "https://github.com/pallets/itsdangerous/",
            "https://www.github.com/pallets/itsdangerous/tree/main/src",
            "github.com/pallets/itsdangerous",
            "git@github.com:pallets/itsdangerous.git",
        ] {
            assert_eq!(parse_source(t), Ok(gh("pallets", "itsdangerous")), "{t}");
        }
        assert_eq!(parse_source("a-b/c.d_e"), Ok(gh("a-b", "c.d_e")));
        assert_eq!(
            parse_source("https://gitlab.com/group/sub/proj.git"),
            Ok(Source::Git { url: "https://gitlab.com/group/sub/proj.git".into(), host: "gitlab.com".into(), owner: "sub".into(), name: "proj".into() })
        );
    }

    #[test]
    fn refuses_local_paths_other_schemes_and_shell() {
        for t in [
            "",
            "/etc/passwd",
            "~/src/shop",
            "./shop",
            "../x/y",
            "file:///tmp/repo",
            "ssh://git@github.com/a/b",
            "ext::sh -c touch% /tmp/pwned",
            "--upload-pack=touch/x",
            "-o/x",
            "a/b; rm -rf ~",
            "a/b$(id)",
            "a/`id`",
            "a /b",
            "a/b\nc",
            "a/..",
            "a/.git",
            "a",
            "a/b/c",
            "-a/b",
            "a_b/c",
            "https://user:pw@github.com/a/b",
            "https://github.com:22/a/b",
            "https://localhost/a/b",
            "https://example.com/a/../b",
            "http://github.com/a/b",
            "C:\\repo",
        ] {
            assert!(parse_source(t).is_err(), "{t:?} was taken: {:?}", parse_source(t));
        }
        assert!(parse_source(&format!("{}/b", "o".repeat(40))).is_err());
    }

    #[test]
    fn detects_each_kind_of_project() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        assert_eq!(detect(p)["test_cmd"], Json::Null);
        std::fs::write(p.join("pyproject.toml"), "[project]\nname = \"x\"\n[project.scripts]\nxtool = \"x.cli:main\"\n[tool.pytest.ini_options]\n").unwrap();
        let v = detect(p);
        assert_eq!(v["language"], "Python");
        assert!(v["test_cmd"].as_str().unwrap().ends_with("python -m pytest -q"), "{v}");
        assert!(v["run_cmd"].as_str().unwrap().ends_with("xtool"), "{v}");
        assert_eq!(v["ci"], false);
        std::fs::create_dir_all(p.join(".github/workflows")).unwrap();
        std::fs::write(p.join(".github/workflows/ci.yml"), "on: push\n").unwrap();
        assert_eq!(detect(p)["ci"], true);
        std::fs::write(p.join("package.json"), r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1","start":"node ."}}"#).unwrap();
        let v = detect(p);
        assert_eq!(v["language"], "JavaScript");
        assert_eq!(v["run_cmd"], "npm start");
        assert!(v["test_cmd"].as_str().unwrap().contains("pytest"), "npm's placeholder test is no test: {v}");
        std::fs::write(p.join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).unwrap();
        assert_eq!(detect(p)["test_cmd"], "npm test");
        std::fs::write(p.join("go.mod"), "module x\n").unwrap();
        assert_eq!(detect(p)["language"], "Go");
        std::fs::write(p.join("Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(detect(p)["test_cmd"], "cargo test");
        let e = tempfile::tempdir().unwrap();
        std::fs::write(e.path().join("Makefile"), "all:\n\ttrue\ntest:\n\t./t.sh\n").unwrap();
        assert_eq!(detect(e.path())["test_cmd"], "make test");
    }
}
