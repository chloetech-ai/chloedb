//! A capsule with every pull request (SPEC.md, "Fourth build", use case
//! 11). `POST /api/agents/:id/capsule` runs `chloectx pack` in the agent's
//! worktree into `<session dir>/capsule`: the decisions, rules, facts,
//! prompts and answers ChloeCtx kept since the agent started (`pack` has
//! no session filter, so it is the repository's memory over that time).
//! `GET /api/agents/:id/pr` drafts the pull request: a title, and a body
//! with a summary, the decisions behind the change (◆ ids), its blast
//! radius, the tests it ran, and how to ask the capsule (`chloectx
//! mount`); the body is saved to `<session dir>/pr.md` for `gh pr create
//! --body-file`. Workbench never opens the pull request itself.
//! `POST /api/agents/:id/capsule/ask` asks the capsule (`chloectx mount`
//! over stdio, its `capsule_ask` tool: search, no model), or, with no
//! capsule yet, `chloectx ask --json` in the worktree.

use std::collections::{BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path as AxPath, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use serde_json::{json, Value as Json};

use crate::blast::{self, git_out, plural};
use crate::db::{iso, now_ms, Agent};
use crate::server::{blocking, parse_body, ApiResult, App};
use crate::supervisor::{ApiError, Supervisor};
use crate::trail::{self, Ran, Sessions};

/// How long packing may take (it copies the store and opens its embedder).
const PACK_TIMEOUT: Duration = Duration::from_secs(600);
/// How long asking the capsule may take (mounting checks its hash).
const ASK_TIMEOUT: Duration = Duration::from_secs(120);

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/agents/:id/capsule", get(get_route).post(pack_route))
        .route("/api/agents/:id/capsule/ask", post(ask_route))
        .route("/api/agents/:id/pr", get(pr_route))
}

async fn get_route(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || get_capsule(&sup, &id)).await?).into_response())
}

async fn pack_route(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || pack(&sup, &id)).await?).into_response())
}

#[derive(serde::Deserialize)]
struct AskB {
    question: Option<String>,
}

async fn ask_route(State(app): State<App>, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    let b: AskB = parse_body(&body)?;
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || ask(&sup, &id, b.question.as_deref().unwrap_or(""))).await?).into_response())
}

async fn pr_route(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || pr(&sup, &id)).await?).into_response())
}

fn agent_of(sup: &Supervisor, id: &str) -> Result<Agent, ApiError> {
    sup.agent_row(id).ok_or_else(|| ApiError::not_found(format!("no agent {id}")))
}

/// Where ChloeCtx runs for an agent: its worktree, else its repository.
fn place(a: &Agent) -> PathBuf {
    let cwd = PathBuf::from(&a.cwd);
    if cwd.is_dir() {
        cwd
    } else {
        PathBuf::from(&a.repo)
    }
}

fn capsule_dir(sup: &Supervisor, id: &str) -> PathBuf {
    sup.session_dir(id).join("capsule")
}

fn mark_key(id: &str) -> String {
    format!("capsule:{id}")
}

/// A span as people say it: 52 min, 3 h 5 min.
fn span(ms: i64) -> String {
    let m = (ms / 60_000).max(1);
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {} min", m / 60, m % 60)
    }
}

fn first_sentence(s: &str, max: usize) -> String {
    let s = s.trim();
    let end = s.find(['.', '\n', '!', '?']).unwrap_or(s.len());
    crate::supervisor::one_line(&s[..end], max)
}

/// The capsule kept for an agent, if its directory is still there.
fn stored(sup: &Supervisor, id: &str) -> Option<Json> {
    let v: Json = serde_json::from_str(&sup.mark(&mark_key(id))?).ok()?;
    Path::new(v["dir"].as_str()?).join("manifest.json").is_file().then_some(v)
}

/// `GET /api/agents/:id/capsule`.
pub fn get_capsule(sup: &Supervisor, id: &str) -> Result<Json, ApiError> {
    agent_of(sup, id)?;
    Ok(json!({ "available": true, "capsule": stored(sup, id) }))
}

/// The capsule as the API answers it, from `chloectx pack --json`'s manifest.
fn capsule_json(a: &Agent, dir: &Path, m: &Json) -> Json {
    let d = dir.to_string_lossy();
    let started = a.started_ms.unwrap_or(a.created);
    json!({
        "dir": d,
        "title": m["title"],
        "records": m["records"],
        "by_kind": m["by_kind"],
        "bytes": m["bytes"],
        "sha256": m["sha256"],
        "created": m["created_ts"].as_i64().map(|t| iso(t * 1000)).unwrap_or_else(|| iso(now_ms())),
        "since": m["since_ts"].as_i64().map(|t| iso(t * 1000)),
        "pack_ms": m["total_ms"].as_f64().or(m["pack_ms"].as_f64()),
        "scope": format!("the repository's memory since {} started ({}), its session among them", a.name, span(now_ms() - started)),
        "mount": format!("chloectx mount {d}"),
        "claude": format!("claude mcp add {}-capsule -- chloectx mount {d}", a.name),
    })
}

/// `POST /api/agents/:id/capsule`: packs it again.
pub fn pack(sup: &Supervisor, id: &str) -> Result<Json, ApiError> {
    let a = agent_of(sup, id)?;
    let out = capsule_dir(sup, id);
    std::fs::create_dir_all(sup.session_dir(id)).map_err(|e| ApiError::internal(format!("cannot create {}: {e}", sup.session_dir(id).display())))?;
    // `pack` wants a new directory: the old capsule waits aside until the
    // new one is there.
    let old = out.with_file_name(format!("capsule.old-{}", now_ms()));
    let had = out.exists();
    if had {
        std::fs::rename(&out, &old).map_err(|e| ApiError::internal(format!("cannot move {} aside: {e}", out.display())))?;
    }
    let restore = || {
        if had {
            let _ = std::fs::remove_dir_all(&out);
            let _ = std::fs::rename(&old, &out);
        }
    };
    let started = a.started_ms.unwrap_or(a.created);
    // A minute before it started, in days (`--since-days` takes a fraction).
    let days = ((now_ms() - started + 60_000).max(60_000) as f64) / 86_400_000.0;
    let title = format!("{}: {}", a.name, first_sentence(a.task.as_deref().unwrap_or(""), 60));
    let out_s = out.to_string_lossy().to_string();
    let days_s = format!("{days:.6}");
    let m = match trail::run_chloectx(&sup.cfg.chloectx_bin, &place(&a), &["pack", &out_s, "--since-days", &days_s, "--title", title.trim_end_matches(": "), "--json"], PACK_TIMEOUT) {
        Ran::Out(m) => m,
        Ran::Missing => {
            restore();
            return Ok(json!({ "available": false, "why": format!("{}. Install it (./install.sh in the ChloeDB checkout) to pack capsules.", trail::not_installed(&sup.cfg.chloectx_bin)) }));
        }
        Ran::Failed(e) => {
            restore();
            return Ok(json!({ "available": false, "why": e }));
        }
    };
    if !out.join("manifest.json").is_file() {
        restore();
        return Ok(json!({ "available": false, "why": format!("chloectx pack wrote no capsule in {out_s}") }));
    }
    if had {
        let _ = std::fs::remove_dir_all(&old);
    }
    let c = capsule_json(&a, &out, &m);
    sup.set_mark(&mark_key(id), &c.to_string())?;
    Ok(json!({ "available": true, "capsule": c }))
}

/// Asks `chloectx mount <dir>` (MCP over stdio) its `capsule_ask` tool:
/// the text it answered, or why not.
fn ask_mounted(bin: &str, dir: &Path, question: &str) -> Result<String, String> {
    let mut child = Command::new(bin).arg("mount").arg(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("cannot run {bin} mount: {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut s);
        s
    });
    let msgs = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "chloedb-workbench", "version": env!("CARGO_PKG_VERSION") } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "capsule_ask", "arguments": { "question": question, "budget": 3000 } } }),
    ];
    for m in &msgs {
        if writeln!(stdin, "{m}").is_err() {
            break;
        }
    }
    let _ = stdin.flush();
    let end = Instant::now() + ASK_TIMEOUT;
    let mut answer: Option<Result<String, String>> = None;
    while answer.is_none() {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(line) => {
                let Ok(v) = serde_json::from_str::<Json>(&line) else { continue };
                if v["id"] != 2 {
                    continue;
                }
                answer = Some(if let Some(e) = v.get("error") {
                    Err(e["message"].as_str().unwrap_or("error").to_string())
                } else {
                    let text = v["result"]["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
                    if v["result"]["isError"] == true {
                        Err(text)
                    } else {
                        Ok(text)
                    }
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => answer = Some(Err(format!("chloectx mount did not answer within {} s", ASK_TIMEOUT.as_secs()))),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let err = err_reader.join().unwrap_or_default();
    answer.unwrap_or_else(|| {
        let why = err.lines().last().unwrap_or("").trim_start_matches("chloectx mount: ").trim().to_string();
        Err(if why.is_empty() { "chloectx mount ended without an answer".into() } else { format!("chloectx mount failed: {why}") })
    })
}

/// The record ids an answer names: the trailer `(capsule "…", records: a, b)`
/// and `[id]` marks.
fn ids_in(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for l in text.lines() {
        if let Some(i) = l.find("records: ") {
            if l.trim_start().starts_with("(capsule") {
                ids.extend(l[i + 9..].trim_end_matches(')').split(", ").map(|s| s.trim().to_string()).filter(|s| s.contains(':')));
            }
        }
        let mut rest = l;
        while let Some(i) = rest.find('[') {
            let Some(j) = rest[i..].find(']') else { break };
            let id = &rest[i + 1..i + j];
            if id.contains(':') && !id.contains(' ') && id.len() < 40 {
                ids.push(id.to_string());
            }
            rest = &rest[i + j..];
        }
    }
    let mut seen = HashSet::new();
    ids.retain(|i| seen.insert(i.clone()));
    ids
}

/// `POST /api/agents/:id/capsule/ask`.
pub fn ask(sup: &Supervisor, id: &str, question: &str) -> Result<Json, ApiError> {
    let a = agent_of(sup, id)?;
    let q = question.trim();
    if q.is_empty() {
        return Err(ApiError::bad("question is empty"));
    }
    let t0 = Instant::now();
    let bin = sup.cfg.chloectx_bin.clone();
    let s = Sessions::load(sup);
    let agents_of = |ids: &[String]| -> Json {
        let mut m = serde_json::Map::new();
        for i in ids {
            if let Some(ag) = s.of_record(i) {
                m.insert(i.clone(), trail::agent_json(sup, ag));
            }
        }
        Json::Object(m)
    };
    if let Some(c) = stored(sup, id) {
        let dir = PathBuf::from(c["dir"].as_str().unwrap_or(""));
        return Ok(match ask_mounted(&bin, &dir, q) {
            Ok(text) => {
                let ids = ids_in(&text);
                json!({ "available": true, "via": "capsule", "question": q, "text": text, "ids": ids, "agents": agents_of(&ids), "ms": t0.elapsed().as_millis() as u64, "missing": [] })
            }
            Err(e) if e.contains("No such file") || e.starts_with("cannot run") => json!({ "available": false, "why": format!("{e}. {}", trail::not_installed(&bin)) }),
            Err(e) => json!({ "available": false, "why": e }),
        });
    }
    // No capsule: the repository's memory, which `ask` reads whole.
    Ok(match trail::run_chloectx(&bin, &place(&a), &["ask", "--json", "--", q], Duration::from_secs(300)) {
        Ran::Out(v) => {
            let text = v["text"].as_str().or(v["answer"].as_str()).unwrap_or("").to_string();
            let mut ids: Vec<String> = v["ids"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
            if ids.is_empty() {
                ids = ids_in(&text);
            }
            let mine: Vec<&String> = ids.iter().filter(|i| s.of_record(i).is_some_and(|ag| ag.id == a.id)).collect();
            json!({
                "available": true, "via": "repo", "question": q, "text": text, "ids": ids, "agents": agents_of(&ids), "ms": t0.elapsed().as_millis() as u64,
                "missing": [format!("There is no capsule yet, so the repository's memory answered (all sessions; {} from {}'s).", if mine.len() == 1 { "1 of these records is".to_string() } else { format!("{} of these records are", mine.len()) }, a.name)],
            })
        }
        Ran::Missing => json!({ "available": false, "why": trail::not_installed(&bin) }),
        Ran::Failed(e) => json!({ "available": false, "why": e }),
    })
}

/// The test runs an agent made, from its Bash calls: command, ok (or null
/// when it cannot be told), a summary line, when.
fn tests_run(sup: &Supervisor, id: &str) -> Vec<Json> {
    let evs = sup.all_events(id).unwrap_or_default();
    let mut out = Vec::new();
    for ev in &evs {
        if ev.tool.as_deref() != Some("Bash") || !matches!(ev.kind.as_str(), "PostToolUse" | "PostToolUseFailure") {
            continue;
        }
        let Some(d) = trail::data_of(ev) else { continue };
        let cmd = d["tool_input"]["command"].as_str().unwrap_or("");
        if !crate::flow::is_test(cmd) {
            continue;
        }
        let r = &d["tool_response"];
        let text = [r["stdout"].as_str(), r["stderr"].as_str(), r.as_str(), r["output"].as_str()].into_iter().flatten().collect::<Vec<_>>().join("\n");
        let failed_word = text.lines().any(|l| {
            let w: Vec<&str> = l.split(|c: char| !c.is_alphanumeric()).filter(|x| !x.is_empty()).collect();
            w.windows(2).any(|p| p[1] == "failed" && p[0].parse::<u64>().is_ok_and(|n| n > 0)) || l.contains("FAILED") || l.contains("FAIL ")
        });
        let summary = text.lines().rev().find(|l| l.contains("test result:") || l.contains("passed") || l.contains("failed") || l.contains("Tests:")).map(|l| crate::supervisor::one_line(l.trim(), 140));
        let ok = if ev.kind == "PostToolUseFailure" || r["is_error"] == true || r["interrupted"] == true || failed_word {
            Some(false)
        } else if summary.is_some() {
            Some(true)
        } else {
            None
        };
        out.push(json!({ "command": crate::supervisor::one_line(cmd, 160), "ok": ok, "summary": summary, "ts": iso(ev.ts) }));
    }
    out
}

/// The agent's session's own decisions: `chloectx sql --json` in its
/// worktree, records whose session is one of its sessions.
fn session_decisions(sup: &Supervisor, a: &Agent, missing: &mut Vec<String>) -> Vec<Json> {
    let since = a.started_ms.unwrap_or(a.created) / 1000 - 60;
    let sql = format!("SELECT id, kind, text, ts, session, tag, links FROM memory WHERE (kind = 'decision' OR kind = 'preference' OR kind = 'fact') AND ts >= {since} ORDER BY ts LIMIT 500");
    let out = match trail::run_chloectx(&sup.cfg.chloectx_bin, &place(a), &["sql", "--json", &sql], Duration::from_secs(60)) {
        Ran::Out(v) => v,
        Ran::Missing => {
            missing.push(trail::not_installed(&sup.cfg.chloectx_bin));
            return Vec::new();
        }
        Ran::Failed(e) => {
            missing.push(e);
            return Vec::new();
        }
    };
    let s = Sessions::load(sup);
    let cols: Vec<String> = out["columns"].as_array().into_iter().flatten().map(|c| c.as_str().or_else(|| c["name"].as_str()).unwrap_or("").to_string()).collect();
    let mut list = Vec::new();
    for row in out["rows"].as_array().into_iter().flatten().filter_map(Json::as_array) {
        let col = |n: &str| cols.iter().position(|c| c == n).and_then(|i| row.get(i).cloned()).unwrap_or(Json::Null);
        let rid = col("id").as_str().unwrap_or("").to_string();
        let mine = col("session").as_str().and_then(|sid| s.agent(sid)).or_else(|| s.of_record(&rid)).is_some_and(|ag| ag.id == a.id);
        if !mine || rid.is_empty() {
            continue;
        }
        let links = col("links").to_string();
        let superseded_by = links.split("superseded_by").nth(1).and_then(|r| r.split('"').find(|p| p.contains(':') && !p.contains(' ') && p.len() > 2)).map(|x| x.trim_start_matches(':').to_string());
        list.push(json!({
            "id": rid, "kind": col("kind"), "text": col("text"), "ts": trail::ctx_time(&col("ts")),
            "superseded": col("tag") == "superseded" || superseded_by.is_some(), "superseded_by": superseded_by, "via": ["session"],
        }));
    }
    list
}

/// `GET /api/agents/:id/pr`: the pull request's draft.
pub fn pr(sup: &Supervisor, id: &str) -> Result<Json, ApiError> {
    let a = agent_of(sup, id)?;
    let b = blast::blast(sup, id)?;
    let dir = place(&a);
    let mut missing: Vec<String> = b["missing"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();

    // Title: its outcome's, else its task's first sentence.
    let outcome = a.outcome.as_deref().and_then(|o| {
        let rows = sup.db().query("SELECT title FROM outcomes WHERE id = :id", &chloedb::query::Params::new().set("id", chloedb::Value::Str(o.to_string()))).ok()?;
        rows.first().and_then(|r| crate::db::get_str(&r[0]))
    });
    let title = outcome.unwrap_or_else(|| first_sentence(a.task.as_deref().unwrap_or(""), 72));
    let title = if title.is_empty() { format!("Changes from {}", a.name) } else { title };
    let main = crate::git::checkout(&dir).map(|c| c.main).unwrap_or_else(|_| PathBuf::from(&a.repo));
    let base_branch = crate::git::current_branch(&main).filter(|b| Some(b) != a.branch.as_ref()).unwrap_or_else(|| "main".into());
    let branch = a.branch.clone().or_else(|| crate::git::current_branch(&dir)).unwrap_or_else(|| "HEAD".into());
    let base = b["base"].as_str().unwrap_or("").to_string();
    let commits: Vec<Json> = git_out(&dir, &["log", "--format=%h%x09%s", &format!("{base}..HEAD")])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(sha, s)| json!({ "sha": sha, "subject": s }))
        .collect();
    let t = &b["totals"];
    let n = |k: &str| t[k].as_u64().unwrap_or(0) as usize;

    // The decisions behind it: its session's, then those about its lines.
    let mut decisions = session_decisions(sup, &a, &mut missing);
    let mut seen: BTreeSet<String> = decisions.iter().filter_map(|d| d["id"].as_str().map(str::to_string)).collect();
    for d in b["decisions"].as_array().into_iter().flatten() {
        let did = d["id"].as_str().unwrap_or("").to_string();
        if seen.insert(did.clone()) {
            decisions.push(json!({ "id": did, "kind": d["kind"], "text": d["text"], "ts": d["ts"], "superseded": d["superseded"], "superseded_by": d["superseded_by"], "via": d["via"] }));
        }
    }
    let tests = tests_run(sup, id);
    let capsule = stored(sup, id);

    // The body.
    let mut body = String::new();
    body += "## Summary\n\n";
    if let Some(task) = a.task.as_deref().filter(|t| !t.trim().is_empty()) {
        body += &format!("{}\n\n", crate::supervisor::one_line(task, 400));
    }
    for c in commits.iter().take(12) {
        body += &format!("- {} (`{}`)\n", c["subject"].as_str().unwrap_or(""), c["sha"].as_str().unwrap_or(""));
    }
    if commits.is_empty() {
        for f in b["files"].as_array().into_iter().flatten().take(12) {
            body += &format!("- `{}` +{} −{}\n", f["path"].as_str().unwrap_or(""), f["added"], f["removed"]);
        }
    }
    body += &format!("\n{}, +{} −{}, on `{branch}` from `{}`.\n\n", plural(n("files"), "file"), n("added"), n("removed"), &base[..base.len().min(7)]);
    body += "## Decisions behind it\n\n";
    if decisions.is_empty() {
        body += "ChloeCtx holds no decision from this work or about these lines.\n\n";
    }
    for d in &decisions {
        let did = d["id"].as_str().unwrap_or("");
        let kind = d["kind"].as_str().unwrap_or("decision");
        let label = match d["superseded_by"].as_str() {
            Some(by) => format!("(superseded by `{by}`)"),
            None if d["superseded"] == true => "(superseded)".into(),
            None => format!("{kind}:"),
        };
        body += &format!("- ◆ `{did}` {label} {}\n", d["text"].as_str().unwrap_or("").trim());
    }
    if !decisions.is_empty() {
        body += "\n";
    }
    body += "## Blast radius\n\n";
    body += &format!("{}\n\n", b["risk"]["line"].as_str().unwrap_or(""));
    let fns: Vec<String> = b["functions"].as_array().into_iter().flatten().map(|f| {
        let name = f["name"].as_str().unwrap_or("");
        match f["status"].as_str() {
            Some("added") => format!("`{name}` (new)"),
            Some("deleted") => format!("`{name}` (removed)"),
            _ => format!("`{name}`"),
        }
    }).collect();
    if !fns.is_empty() {
        body += &format!("- Functions: {}\n", fns.join(", "));
    }
    let ts: Vec<String> = b["tests"].as_array().into_iter().flatten().take(8).map(|t| {
        let r = &t["runs"];
        let runs = if r.is_null() {
            "not run".to_string()
        } else {
            format!("{} of {}{}", r["passed"], r["total"], if r["flaky"] == true { ", flaky" } else { "" })
        };
        format!("`{}` ({runs})", t["path"].as_str().unwrap_or(""))
    }).collect();
    if !ts.is_empty() {
        body += &format!("- Tests that cover them: {}\n", ts.join(", "));
    }
    let ags: Vec<String> = b["agents"].as_array().into_iter().flatten().map(|g| g["detail"].as_str().unwrap_or("").to_string()).collect();
    if !ags.is_empty() {
        body += &format!("- Other agents: {}\n", ags.join("; "));
    }
    body += "\n## Tests run\n\n";
    if tests.is_empty() {
        body += "The agent ran no tests that Workbench saw.\n";
    }
    for t in &tests {
        let how = match t["ok"].as_bool() {
            Some(true) => "passed",
            Some(false) => "failed",
            None => "ran",
        };
        body += &format!("- `{}`: {how}{}\n", t["command"].as_str().unwrap_or(""), t["summary"].as_str().map(|s| format!(" ({s})")).unwrap_or_default());
    }
    body += "\n## Ask why\n\n";
    match &capsule {
        Some(c) => {
            body += &format!(
                "This pull request carries a capsule of the work behind it: {} packed by ChloeCtx from {}'s session. Ask it instead of guessing:\n\n```\n{}\n```\n\nthen, in Claude Code, `capsule_ask` with the question (\"why was it done this way?\"). It answers from the records, with no model call.\n",
                plural(c["records"].as_u64().unwrap_or(0) as usize, "record"),
                a.name,
                c["claude"].as_str().unwrap_or("")
            );
        }
        None => {
            body += "No capsule was packed for it yet (Workbench's Ship screen packs one with `chloectx pack`).\n";
            missing.push("No capsule yet: pack one to give the pull request its Ask why section.".into());
        }
    }
    let body_file = sup.session_dir(id).join("pr.md");
    if let Err(e) = std::fs::create_dir_all(sup.session_dir(id)).and_then(|_| std::fs::write(&body_file, &body)) {
        missing.push(format!("The body could not be saved to {}: {e}", body_file.display()));
    }
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`"));
    let gh = format!("gh pr create --base {base_branch} --head {branch} --title {} --body-file {}", q(&title), body_file.display());
    Ok(json!({
        "agent": trail::agent_json(sup, &a), "title": title, "branch": branch, "base_branch": base_branch, "base": base,
        "body": body, "body_file": body_file.to_string_lossy(), "gh": gh, "push": format!("git push -u origin {branch}"),
        "sections": {
            "summary": commits.iter().filter_map(|c| c["subject"].as_str().map(str::to_string)).collect::<Vec<_>>(),
            "commits": commits, "decisions": decisions,
            "blast": { "level": b["risk"]["level"], "line": b["risk"]["line"], "functions": n("functions"), "tests": n("tests"), "agents": b["agents"].as_array().into_iter().flatten().filter_map(|g| g["agent"]["name"].as_str()).collect::<Vec<_>>() },
            "tests": tests,
        },
        "capsule": capsule, "missing": missing,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_record_ids_from_an_answer() {
        let t = "Rules:\n- [e3b1c2d4:212] decision: per line.\n\n(capsule \"checkout\" sha256:9b1e0c47d2a3, records: e3b1c2d4:212, e3b1c2d4:198)";
        assert_eq!(ids_in(t), ["e3b1c2d4:212", "e3b1c2d4:198"]);
        assert_eq!(first_sentence("Charge tax by address. Then more", 72), "Charge tax by address");
    }
}
