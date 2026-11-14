//! Workbench end to end, with a fake `claude`: a shell script that prints
//! its arguments, runs the hooks of the settings file it was given (so the
//! agent's state changes as a real session's would), and answers lines
//! typed into its terminal. No model is called.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_chloedb-workbench");
const TOKEN: &str = "test-token-0123456789";

/// The fake Claude Code. Its hooks are the ones the settings file names.
const FAKE: &str = r#"#!/bin/bash
while [ $# -gt 0 ]; do
  case "$1" in
    --session-id) SID="$2"; shift 2;;
    --settings) SETTINGS="$2"; shift 2;;
    # As Claude Code's: --mcp-config takes every word up to the next
    # option, so a task that follows it must come after `--`.
    --mcp-config) MCP="$2"; shift 2
      while [ $# -gt 0 ] && [ "${1#-}" = "$1" ]; do echo "MCP config file not found: $1"; exit 1; done;;
    --model) MODEL="$2"; shift 2;;
    --) TASK="$2"; shift $#;;
    *) TASK="$1"; shift;;
  esac
done
echo "FAKE CLAUDE sid=$SID model=$MODEL agent=$CHLOEDB_WORKBENCH_AGENT"
echo "TASK: $TASK"
echo "CWD: $(pwd)"
hook() {
  local cmd
  cmd=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["hooks"][sys.argv[2]][0]["hooks"][0]["command"])' "$SETTINGS" "$1")
  printf '%s' "$2" | eval "$cmd"
}
hook SessionStart '{"hook_event_name":"SessionStart","source":"startup","session_id":"'"$SID"'"}'
turn() {
  hook UserPromptSubmit '{"hook_event_name":"UserPromptSubmit","prompt":"'"$1"'","session_id":"'"$SID"'"}'
  hook PreToolUse '{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"echo hi"},"session_id":"'"$SID"'"}'
  case "$1" in
    *ASK*)
      hook Notification '{"hook_event_name":"Notification","message":"Claude needs your permission to use Bash","session_id":"'"$SID"'"}'
      read -r answer
      echo "ANSWER: $answer";;
  esac
  hook PostToolUse '{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"echo hi"},"session_id":"'"$SID"'"}'
  echo "DONE: $1"
  hook Stop '{"hook_event_name":"Stop","session_id":"'"$SID"'"}'
}
[ -n "$TASK" ] && turn "$TASK"
while read -r line; do
  echo "GOT: $line"
  case "$line" in
    exit) hook SessionEnd '{"hook_event_name":"SessionEnd","reason":"prompt_input_exit","session_id":"'"$SID"'"}'; exit 3;;
    size) echo "SIZE: $(stty size)";;
    *) turn "$line";;
  esac
done
"#;

struct Server {
    child: Child,
    url: String,
    data: PathBuf,
    _dir: tempfile::TempDir,
}

fn http() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).http_status_as_error(false).proxy(None).build().into()
}

fn write_fake(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-claude");
    std::fs::write(&p, FAKE).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

impl Server {
    fn start(args: &[&str]) -> Server {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        Server::start_in(dir, data, args)
    }

    fn start_in(dir: tempfile::TempDir, data: PathBuf, args: &[&str]) -> Server {
        let fake = write_fake(dir.path());
        let mut child = Command::new(BIN)
            .args(["serve", "--addr", "127.0.0.1:0", "--token", TOKEN])
            .arg("--data-dir")
            .arg(&data)
            .arg("--claude-bin")
            .arg(&fake)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        let url = line.trim().strip_prefix("Workbench: ").unwrap_or_else(|| panic!("no URL in {line:?}")).split("/?token=").next().unwrap().to_string();
        std::thread::spawn(move || {
            let mut sink = String::new();
            let _ = out.read_to_string(&mut sink);
        });
        Server { child, url, data, _dir: dir }
    }

    fn get(&self, path: &str) -> (u16, Value) {
        let mut r = http().get(format!("{}{path}", self.url)).header("authorization", format!("Bearer {TOKEN}")).call().unwrap();
        let status = r.status().as_u16();
        (status, r.body_mut().read_json().unwrap_or(Value::Null))
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let mut r = http().post(format!("{}{path}", self.url)).header("authorization", format!("Bearer {TOKEN}")).send_json(body).unwrap();
        let status = r.status().as_u16();
        (status, r.body_mut().read_json().unwrap_or(Value::Null))
    }

    fn spawn(&self, body: Value) -> Value {
        let (status, v) = self.post("/api/agents", body);
        assert_eq!(status, 201, "{v}");
        v["agent"].clone()
    }

    fn agent(&self, id: &str) -> Value {
        self.get(&format!("/api/agents/{id}")).1["agent"].clone()
    }

    fn wait(&self, id: &str, until: &str) -> Value {
        let (status, v) = self.get(&format!("/api/wait?agent={id}&until={until}&timeout_s=30"));
        assert_eq!(status, 200, "{v}");
        assert_eq!(v["reached"], json!(true), "{id} did not reach {until}: {v}");
        v["agent"].clone()
    }

    fn events(&self, id: &str) -> Vec<Value> {
        self.get(&format!("/api/agents/{id}/events?data=1")).1["events"].as_array().cloned().unwrap()
    }

    /// Polls until `f` of the agent holds.
    fn until(&self, id: &str, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let a = self.agent(id);
            if f(&a) {
                return a;
            }
            assert!(Instant::now() < end, "{id}: {what} never held: {a}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// SIGTERM, as a person's Ctrl-C would, and waits for it to exit.
    fn stop(mut self) -> (tempfile::TempDir, PathBuf) {
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < end, "the server did not stop");
            std::thread::sleep(Duration::from_millis(50));
        }
        let dir = std::mem::replace(&mut self._dir, tempfile::tempdir().unwrap());
        (dir, self.data.clone())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let end = Instant::now() + Duration::from_secs(15);
        while Instant::now() < end {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A git repository with one commit, on branch `main`.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().join("proj");
    std::fs::create_dir(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    std::fs::write(r.join("README"), "hello\n").unwrap();
    git(&r, &["add", "README"]);
    git(&r, &["commit", "-q", "-m", "first"]);
    let r = r.canonicalize().unwrap();
    (dir, r)
}

fn kinds(evs: &[Value]) -> Vec<String> {
    evs.iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn spawns_in_a_worktree_and_hooks_set_its_state() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "name": "hi", "model": "sonnet" }));
    let id = a["id"].as_str().unwrap().to_string();
    assert!(id.starts_with('a') && id.len() == 9, "{id}");
    assert_eq!(a["name"], "hi");
    assert_eq!(a["branch"], "wb/hi");
    assert_eq!(a["repo"], json!(r));
    assert_eq!(a["depth"], 0);
    assert_eq!(a["running"], true);
    let cwd = PathBuf::from(a["cwd"].as_str().unwrap());
    assert!(cwd.starts_with(s.data.canonicalize().unwrap().join("worktrees")), "{cwd:?}");
    assert!(cwd.ends_with("hi"));
    assert!(cwd.join("README").exists());
    assert!(git(&r, &["worktree", "list"]).contains(cwd.to_str().unwrap()));

    let a = s.wait(&id, "idle");
    assert_eq!(a["state"], "idle");
    assert_eq!(a["last_event"], "turn finished");
    let evs = s.events(&id);
    assert_eq!(kinds(&evs), ["spawned", "SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"]);
    assert_eq!(evs[3]["tool"], "Bash");
    assert_eq!(evs[3]["summary"], "Bash: echo hi");
    assert!(evs[3]["data"].as_str().unwrap().contains("\"tool_input\""));
    let seqs: Vec<i64> = evs.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(evs[0]["id"].as_str().unwrap(), format!("{id}:{:012}", seqs[0]));
    // Without ?data=1, no data; after= skips.
    let (_, v) = s.get(&format!("/api/agents/{id}/events?after={}&limit=2", seqs[1]));
    let page = v["events"].as_array().unwrap();
    assert_eq!(kinds(page), ["UserPromptSubmit", "PreToolUse"]);
    assert!(page[0].get("data").is_none());

    // The attention queue: idle agents.
    let (_, att) = s.get("/api/attention");
    assert_eq!(att["agents"][0]["id"], json!(id));

    // The settings and MCP config it was started with.
    let sess = s.data.join("sessions").join(&id);
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(sess.join("settings.json")).unwrap()).unwrap();
    for ev in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Notification", "Stop", "SubagentStop", "SessionEnd"] {
        let h = &settings["hooks"][ev][0]["hooks"][0];
        assert!(h["command"].as_str().unwrap().ends_with(" hook"), "{ev}");
        assert_eq!(h["timeout"], 5);
    }
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(sess.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["workbench"]["args"], json!(["mcp"]));

    // The arguments, in the terminal's scrollback.
    let screen = ws_read_until(&s, &id, "CWD:", &[]);
    let sid = a["session_id"].as_str().unwrap();
    assert!(screen.contains(&format!("FAKE CLAUDE sid={sid} model=sonnet agent={id}")), "{screen}");
    assert!(screen.contains("TASK: say hi"));
    assert!(screen.contains(&format!("CWD: {}", cwd.display())));

    // A name in use is refused; once its agent exited, a new agent of the
    // name picks up its worktree and branch.
    let (status, v) = s.post("/api/agents", json!({ "repo": r, "name": "hi" }));
    assert_eq!(status, 409, "{v}");
    s.post(&format!("/api/agents/{id}/stop"), json!({}));
    s.until(&id, "stopped", |a| a["running"] == false);
    let again = s.spawn(json!({ "repo": r, "name": "hi" }));
    assert_eq!(again["cwd"], json!(cwd));
    assert_eq!(again["branch"], "wb/hi");
    // A branch of its own from a base.
    git(&r, &["branch", "base-x"]);
    let c = s.spawn(json!({ "repo": r, "name": "other", "branch": "feature/x", "base": "base-x" }));
    assert_eq!(c["branch"], "feature/x");
    assert!(git(&r, &["branch", "--list", "feature/x"]).contains("feature/x"));
}

#[test]
fn spawns_without_a_worktree_and_reads_a_worktree_back_to_its_repo() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "worktree": false }));
    assert_eq!(a["cwd"], json!(r));
    assert_eq!(a["repo"], json!(r));
    assert_eq!(a["branch"], "main");
    assert_eq!(a["name"], "agent");
    assert_eq!(a["task"], Value::Null);
    let id = a["id"].as_str().unwrap();
    // No task: started, then nothing until a prompt.
    s.until(id, "working", |a| a["state"] == "working");
    assert_eq!(kinds(&s.events(id)), ["spawned", "SessionStart"]);

    // A worktree passed as repo: the repository is its main checkout.
    let b = s.spawn(json!({ "repo": r, "task": "fix the login bug" }));
    assert_eq!(b["name"], "fix-the-login-bug");
    let c = s.spawn(json!({ "repo": b["cwd"], "task": "fix the login bug", "worktree": false }));
    assert_eq!(c["repo"], json!(r));
    assert_eq!(c["cwd"], b["cwd"]);
    assert_eq!(c["name"], "fix-the-login-bug-2");
    assert_eq!(c["branch"], "wb/fix-the-login-bug");

    // Not a checkout, and a bad name.
    let plain = tempfile::tempdir().unwrap();
    let (status, v) = s.post("/api/agents", json!({ "repo": plain.path() }));
    assert_eq!(status, 400, "{v}");
    assert_eq!(v["error_code"], "bad_request");
    let (status, _) = s.post("/api/agents", json!({ "repo": r, "name": "Bad Name" }));
    assert_eq!(status, 400);
    let (status, _) = s.post("/api/agents", json!({ "repo": "relative/path" }));
    assert_eq!(status, 400);
}

#[test]
fn waiting_input_idle_and_exit() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "ASK first", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    let a = s.wait(&id, "waiting");
    assert_eq!(a["waiting_for"], "Claude needs your permission to use Bash");
    let (_, att) = s.get("/api/attention");
    assert_eq!(att["agents"][0]["waiting_for"], "Claude needs your permission to use Bash");

    // An answer: input, back to working, then the turn ends.
    let (status, _) = s.post(&format!("/api/agents/{id}/input"), json!({ "text": "yes" }));
    assert_eq!(status, 200);
    let a = s.agent(&id);
    assert!(a["state"] == "working" || a["state"] == "idle", "{a}");
    assert_eq!(a["waiting_for"], Value::Null);
    let a = s.wait(&id, "idle");
    assert_eq!(a["state"], "idle");
    let k = kinds(&s.events(&id));
    assert_eq!(k, ["spawned", "SessionStart", "UserPromptSubmit", "PreToolUse", "Notification", "input", "PostToolUse", "Stop"]);
    let screen = ws_read_until(&s, &id, "DONE: ASK first", &[]);
    assert!(screen.contains("ANSWER: yes"), "{screen}");

    // Another turn, then `any` times out with nothing new.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "another turn" }));
    let a = s.wait(&id, "idle");
    assert_eq!(a["last_event"], "turn finished");
    let (_, v) = s.get(&format!("/api/wait?agent={id}&until=any&timeout_s=1"));
    assert_eq!(v["reached"], false);

    // Exit: SessionEnd, then the process's own exit code.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "exit" }));
    s.wait(&id, "exited");
    let a = s.until(&id, "the process exited", |a| a["running"] == false && a["exit_code"] == 3);
    assert_eq!(a["state"], "exited");
    assert!(a["ended"].is_string());
    assert_eq!(*kinds(&s.events(&id)).last().unwrap(), "exited");
    // Exited agents only with all=1.
    let (_, v) = s.get("/api/agents");
    assert!(v["agents"].as_array().unwrap().is_empty(), "{v}");
    let (_, v) = s.get("/api/agents?all=1");
    assert_eq!(v["agents"][0]["id"], json!(id));
    // Input to an exited agent is a conflict.
    let (status, _) = s.post(&format!("/api/agents/{id}/input"), json!({ "text": "x" }));
    assert_eq!(status, 409);
}

#[test]
fn stop_ends_an_agent_and_a_restart_keeps_the_rows() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    let (status, _) = s.post(&format!("/api/agents/{id}/stop"), json!({}));
    assert_eq!(status, 200);
    let a = s.until(&id, "stopped", |a| a["running"] == false);
    assert_eq!(a["state"], "exited");

    let b = s.spawn(json!({ "repo": r, "task": "say hi again", "worktree": false }));
    let bid = b["id"].as_str().unwrap().to_string();
    s.wait(&bid, "idle");
    let n_events = s.events(&bid).len();
    // Stopping the server ends its agents; a new server reads the rows.
    let (dir, data) = s.stop();
    let s = Server::start_in(dir, data, &[]);
    let (_, v) = s.get("/api/agents?all=1");
    let agents = v["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 2, "{v}");
    for a in agents {
        assert_eq!(a["state"], "exited");
        assert_eq!(a["running"], false);
    }
    let evs = s.events(&bid);
    assert!(evs.len() > n_events, "{evs:?}");
    assert_eq!(evs.last().unwrap()["kind"], "exited");
    // Sequence numbers go on from where they were.
    let c = s.spawn(json!({ "repo": r, "worktree": false }));
    let cev = s.events(c["id"].as_str().unwrap());
    assert!(cev[0]["seq"].as_i64().unwrap() > evs.last().unwrap()["seq"].as_i64().unwrap());
}

#[test]
fn limits() {
    let s = Server::start(&["--max-agents", "3", "--max-depth", "2"]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "name": "a", "worktree": false }));
    let b = s.spawn(json!({ "repo": r, "name": "b", "worktree": false, "parent": a["id"] }));
    assert_eq!(b["depth"], 1);
    assert_eq!(b["parent"], a["id"]);
    assert_eq!(s.agent(a["id"].as_str().unwrap())["children"], json!([b["id"]]));
    // An agent started by an agent started by you may start no more.
    let (status, v) = s.post("/api/agents", json!({ "repo": r, "worktree": false, "parent": b["id"] }));
    assert_eq!(status, 409, "{v}");
    assert_eq!(v["error_code"], "conflict");
    // A name in use.
    let (status, v) = s.post("/api/agents", json!({ "repo": r, "name": "a", "worktree": false }));
    assert_eq!(status, 409, "{v}");
    // An unknown parent.
    let (status, _) = s.post("/api/agents", json!({ "repo": r, "worktree": false, "parent": "a00000000" }));
    assert_eq!(status, 404);
    // Three running: the most.
    s.spawn(json!({ "repo": r, "name": "c", "worktree": false }));
    let (status, v) = s.post("/api/agents", json!({ "repo": r, "name": "d", "worktree": false }));
    assert_eq!(status, 429, "{v}");
    assert_eq!(v["error_code"], "limit_exceeded");
    let (_, h) = s.get("/api/health");
    assert_eq!(h["agents_running"], 3);
    assert_eq!(h["max_agents"], 3);
}

#[test]
fn the_token_host_and_origin_are_checked() {
    let s = Server::start(&[]);
    let raw = |method: &str, path: &str, headers: &[(&str, &str)]| -> u16 {
        assert_eq!(method, "GET");
        let mut b = http().get(format!("{}{path}", s.url));
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.call().unwrap().status().as_u16()
    };
    let bearer = format!("Bearer {TOKEN}");
    assert_eq!(raw("GET", "/api/health", &[]), 200);
    assert_eq!(raw("GET", "/api/agents", &[]), 401);
    assert_eq!(raw("GET", "/api/agents", &[("authorization", "Bearer nope")]), 401);
    assert_eq!(raw("GET", "/api/agents", &[("authorization", &bearer)]), 200);
    // ?token= only where a browser cannot set a header.
    assert_eq!(raw("GET", &format!("/api/agents?token={TOKEN}"), &[]), 401);
    // The UI's page needs no token.
    assert_eq!(raw("GET", "/", &[]), 200);
    assert_eq!(raw("GET", "/app.js", &[]), 200);
    assert_eq!(raw("GET", "/style.css", &[]), 200);
    // Another host name (DNS rebinding) is refused.
    let port = s.url.rsplit(':').next().unwrap();
    assert_eq!(raw("GET", "/api/health", &[("host", &format!("evil.example:{port}"))]), 403);
    assert_eq!(raw("GET", "/api/health", &[("host", &format!("localhost:{port}"))]), 200);
    // A change from another origin is refused, one from its own is not.
    let (_d, r) = repo();
    let post = |origin: Option<&str>| -> u16 {
        let mut b = http().post(format!("{}/api/agents", s.url)).header("authorization", &bearer);
        if let Some(o) = origin {
            b = b.header("origin", o);
        }
        b.send_json(json!({ "repo": r, "worktree": false })).unwrap().status().as_u16()
    };
    assert_eq!(post(Some("http://evil.example")), 403);
    assert_eq!(post(Some(&s.url)), 201);
    // No token at all on a change.
    let status = http().post(format!("{}/api/agents", s.url)).send_json(json!({ "repo": r })).unwrap().status().as_u16();
    assert_eq!(status, 401);
    // The stream takes ?token=.
    let mut res = http().get(format!("{}/api/stream?token={TOKEN}", s.url)).call().unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let mut reader = BufReader::new(res.body_mut().as_reader());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "event: snapshot");
    line.clear();
    reader.read_line(&mut line).unwrap();
    let snap: Value = serde_json::from_str(line.trim().strip_prefix("data: ").unwrap()).unwrap();
    assert_eq!(snap["agents"].as_array().unwrap().len(), 1);
    // A change arrives as `event: agent` and `event: event`.
    let id = snap["agents"][0]["id"].as_str().unwrap().to_string();
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "hello" }));
    let mut seen = Vec::new();
    while !(seen.contains(&"event: event".to_string()) && seen.contains(&"event: agent".to_string())) {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line.starts_with("event:") {
            seen.push(line.trim().to_string());
        }
    }
}

/// Connects to an agent's terminal, sends `send` (text frames), and reads
/// until the screen holds `want`.
fn ws_read_until(s: &Server, id: &str, want: &str, send: &[&str]) -> String {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let url = format!("{}/api/agents/{id}/pty?token={TOKEN}", s.url.replace("http://", "ws://"));
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        for m in send {
            ws.send(Message::Text(m.to_string())).await.unwrap();
        }
        let mut screen = String::new();
        let end = tokio::time::Instant::now() + Duration::from_secs(30);
        while !screen.contains(want) {
            match tokio::time::timeout_at(end, ws.next()).await {
                Ok(Some(Ok(Message::Binary(b)))) => screen.push_str(&String::from_utf8_lossy(&b)),
                Ok(Some(Ok(_))) => {}
                other => panic!("no {want:?} in the terminal ({other:?}): {screen}"),
            }
        }
        screen
    })
}

#[test]
fn the_terminal_replays_its_scrollback_and_streams_input_and_output() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    // The scrollback first, then what typing makes.
    let screen = ws_read_until(&s, &id, "GOT: hello there", &["hello there\r"]);
    assert!(screen.contains("FAKE CLAUDE"), "{screen}");
    assert!(screen.contains("DONE: say hi"));
    // A resize message resizes the terminal, and is not typed.
    let screen = ws_read_until(&s, &id, "SIZE: 40 100", &["{\"resize\":[100,40]}", "size\r"]);
    assert!(!screen.contains("GOT: {"), "{screen}");
    // The API's resize too.
    let (status, _) = s.post(&format!("/api/agents/{id}/resize"), json!({ "cols": 90, "rows": 30 }));
    assert_eq!(status, 200);
    let screen = ws_read_until(&s, &id, "SIZE: 30 90", &["size\r"]);
    // A second terminal attached sees the same scrollback.
    assert!(screen.contains("FAKE CLAUDE"));
    // Binary frames are keystrokes too.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let url = format!("{}/api/agents/{id}/pty?token={TOKEN}", s.url.replace("http://", "ws://"));
    rt.block_on(async {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        ws.send(Message::Binary(b"binary keys\r".to_vec())).await.unwrap();
        let mut screen = String::new();
        let end = tokio::time::Instant::now() + Duration::from_secs(30);
        while !screen.contains("GOT: binary keys") {
            match tokio::time::timeout_at(end, ws.next()).await {
                Ok(Some(Ok(Message::Binary(b)))) => screen.push_str(&String::from_utf8_lossy(&b)),
                Ok(Some(Ok(_))) => {}
                other => panic!("{other:?}: {screen}"),
            }
        }
    });
    // No token, no terminal.
    let status = http().get(format!("{}/api/agents/{id}/pty", s.url)).call().unwrap().status().as_u16();
    assert_eq!(status, 401);
}

struct Mcp {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
    next: i64,
}

impl Mcp {
    fn start(s: &Server, caller: &str) -> Mcp {
        let mut child = Command::new(BIN)
            .arg("mcp")
            .env("CHLOEDB_WORKBENCH_URL", &s.url)
            .env("CHLOEDB_WORKBENCH_TOKEN", TOKEN)
            .env("CHLOEDB_WORKBENCH_AGENT", caller)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Mcp { child, out, next: 1 }
    }

    fn send(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], id);
        v
    }

    fn notify(&mut self, method: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "method": method })).unwrap();
    }

    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        let v = self.send("tools/call", json!({ "name": name, "arguments": args }));
        let r = &v["result"];
        (r["content"][0]["text"].as_str().unwrap().to_string(), r["isError"] == true)
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_spawns_lists_and_waits_for_agents() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let parent = s.spawn(json!({ "repo": r, "name": "lead", "worktree": false }));
    let pid = parent["id"].as_str().unwrap().to_string();
    let mut m = Mcp::start(&s, &pid);
    let init = m.send("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "workbench");
    assert!(init["result"]["instructions"].as_str().unwrap().contains("tokens"));
    m.notify("notifications/initialized");
    let tools = m.send("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["spawn_agent", "list_agents", "read_agent", "wait_agent", "send_to_agent", "stop_agent"]);

    let (text, err) = m.tool("spawn_agent", json!({ "task": "ASK the user", "name": "helper" }));
    assert!(!err, "{text}");
    assert!(text.contains("Started agent helper"), "{text}");
    let (_, v) = s.get("/api/agents");
    let child = v["agents"].as_array().unwrap().iter().find(|a| a["name"] == "helper").unwrap().clone();
    let cid = child["id"].as_str().unwrap().to_string();
    assert_eq!(child["parent"], json!(pid));
    assert_eq!(child["depth"], 1);
    assert_eq!(child["repo"], json!(r));
    assert_eq!(child["branch"], "wb/helper");

    let (text, _) = m.tool("list_agents", json!({}));
    assert!(text.contains(&cid) && text.contains("helper"), "{text}");
    assert!(!text.contains("lead"), "only the caller's agents: {text}");
    let (text, _) = m.tool("list_agents", json!({ "all": true }));
    assert!(text.contains("lead"), "{text}");

    let (text, err) = m.tool("wait_agent", json!({ "id": cid, "until": "waiting", "timeout_s": 30 }));
    assert!(!err, "{text}");
    assert!(text.starts_with("reached waiting"), "{text}");
    assert!(text.contains("Claude needs your permission"), "{text}");
    let (text, err) = m.tool("send_to_agent", json!({ "id": cid, "text": "go ahead" }));
    assert!(!err, "{text}");
    let (text, _) = m.tool("wait_agent", json!({ "id": cid, "timeout_s": 30 }));
    assert!(text.starts_with("reached idle"), "{text}");
    assert!(text.contains("turn finished"));
    let (text, _) = m.tool("read_agent", json!({ "id": cid }));
    assert!(text.contains("Notification: Claude needs your permission") && text.contains("input: go ahead"), "{text}");
    let (text, err) = m.tool("stop_agent", json!({ "id": cid }));
    assert!(!err, "{text}");
    let (text, _) = m.tool("wait_agent", json!({ "id": cid, "until": "exited", "timeout_s": 30 }));
    assert!(text.starts_with("reached exited"), "{text}");
    // An error is a tool error, not a protocol one.
    let (text, err) = m.tool("read_agent", json!({ "id": "a00000000" }));
    assert!(err, "{text}");
    // The child may not start agents past the depth.
    let mut mc = Mcp::start(&s, &cid);
    let (text, err) = mc.tool("spawn_agent", json!({ "task": "x" }));
    assert!(err, "{text}");
}

#[test]
fn the_hook_subcommand_never_fails() {
    // No variables: exits 0 at once, prints nothing.
    let out = Command::new(BIN).arg("hook").env_remove("CHLOEDB_WORKBENCH_URL").stdin(Stdio::null()).output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    // A server that is not there: still 0, at most about 2 s.
    let t = Instant::now();
    let mut c = Command::new(BIN)
        .arg("hook")
        .env("CHLOEDB_WORKBENCH_URL", "http://127.0.0.1:9")
        .env("CHLOEDB_WORKBENCH_TOKEN", "x")
        .env("CHLOEDB_WORKBENCH_AGENT", "a1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(b"{\"hook_event_name\":\"Stop\"}").unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(t.elapsed() < Duration::from_secs(5));
}
