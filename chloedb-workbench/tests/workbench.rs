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
    WRITE\ *)
      # WRITE <file> <text>: a line written with the Edit tool.
      local rest="${1#WRITE }"; local file="${rest%% *}"; local text="${rest#* }"
      local edit='{"hook_event_name":"%s","tool_name":"Edit","tool_input":{"file_path":"'"$PWD/$file"'","old_string":"","new_string":"'"$text"'"},"session_id":"'"$SID"'"}'
      echo "EDITING $file: $text"
      hook PreToolUse "$(printf "$edit" PreToolUse)"
      printf '%s\n' "$text" >> "$file"
      hook PostToolUse "$(printf "$edit" PostToolUse)";;
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

// The decision trail.

/// A fake `chloectx`: it prints the canned JSON of `<dir>/<command>.json`
/// and keeps where it ran and its arguments in `<dir>/<command>.args`.
fn fake_chloectx(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-chloectx");
    let script = format!(
        r#"#!/bin/bash
D='{}'
printf '%s\n' "$PWD" "$@" > "$D/$1.args"
[ -f "$D/$1.json" ] || {{ echo "chloectx: no store for $PWD" >&2; exit 1; }}
cat "$D/$1.json"
"#,
        dir.display()
    );
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn args_of(dir: &Path, cmd: &str) -> Vec<String> {
    std::fs::read_to_string(dir.join(format!("{cmd}.args"))).unwrap().lines().map(str::to_string).collect()
}

fn texts(rec: &Value) -> String {
    rec["events"].as_array().unwrap().iter().filter(|e| e[1] == "o").map(|e| e[2].as_str().unwrap()).collect()
}

#[test]
fn the_terminal_is_recorded_and_a_window_starts_from_the_screen_before_it() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    let started = a["started_ms"].as_i64().unwrap();
    assert!(started > 0, "{a}");
    assert_eq!(a["base"].as_str().unwrap(), git(&r, &["rev-parse", "HEAD"]));
    s.wait(&id, "idle");
    std::thread::sleep(Duration::from_millis(150));
    s.post(&format!("/api/agents/{id}/resize"), json!({ "cols": 100, "rows": 40 }));
    std::thread::sleep(Duration::from_millis(150));
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "hello there" }));
    s.wait(&id, "idle");
    ws_read_until(&s, &id, "DONE: hello there", &[]);

    // The whole of it.
    let (status, all) = s.get(&format!("/api/agents/{id}/recording"));
    assert_eq!(status, 200, "{all}");
    assert_eq!((all["width"].as_i64(), all["height"].as_i64(), all["started_ms"].as_i64()), (Some(120), Some(32), Some(started)));
    assert_eq!(all["before"], "");
    let evs = all["events"].as_array().unwrap();
    assert!(texts(&all).contains("FAKE CLAUDE") && texts(&all).contains("GOT: hello there"), "{all}");
    assert!(evs.iter().any(|e| e[1] == "r" && e[2] == "100x40"), "{all}");
    let ms: Vec<i64> = evs.iter().map(|e| e[0].as_i64().unwrap()).collect();
    assert!(ms.windows(2).all(|w| w[0] <= w[1]));
    let got = evs.iter().find(|e| e[1] == "o" && e[2].as_str().unwrap().contains("GOT: hello")).unwrap()[0].as_i64().unwrap();

    // From the moment the line was typed: the screen before it, at its size.
    let (_, w) = s.get(&format!("/api/agents/{id}/recording?from_ms={got}"));
    assert_eq!((w["width"].as_i64(), w["height"].as_i64()), (Some(100), Some(40)), "{w}");
    let before = w["before"].as_str().unwrap();
    assert!(before.contains("DONE: say hi") && !before.contains("GOT: hello"), "{before}");
    assert!(texts(&w).contains("GOT: hello there"));
    assert!(w["events"].as_array().unwrap().iter().all(|e| e[0].as_i64().unwrap() >= got));
    let (_, w) = s.get(&format!("/api/agents/{id}/recording?from_ms=0&to_ms={}", got - 1));
    assert!(!texts(&w).contains("GOT: hello") && texts(&w).contains("DONE: say hi"), "{w}");

    // asciicast v2, kept after the agent exits.
    let cast = s.data.join("sessions").join(&id).join("term.cast");
    let body = std::fs::read_to_string(&cast).unwrap();
    let header: Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
    assert_eq!((header["version"].as_i64(), header["width"].as_i64(), header["height"].as_i64()), (Some(2), Some(120), Some(32)));
    assert_eq!(header["timestamp"].as_i64(), Some(started / 1000));
    s.post(&format!("/api/agents/{id}/stop"), json!({}));
    s.until(&id, "stopped", |a| a["running"] == false);
    let (status, w) = s.get(&format!("/api/agents/{id}/recording"));
    assert_eq!(status, 200);
    assert!(texts(&w).contains("GOT: hello there"));
    assert_eq!(s.get(&format!("/api/agents/{id}/recording?from_ms=5&to_ms=1")).0, 400);
    assert_eq!(s.get("/api/agents/a00000000/recording").0, 404);
    drop(s);

    // Past its cap it stops, and says so.
    let s = Server::start(&["--record-max-bytes", "400"]);
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    let end = Instant::now() + Duration::from_secs(30);
    while !s.events(&id).iter().any(|e| e["kind"] == "recording") {
        assert!(Instant::now() < end, "{:?}", s.events(&id));
        s.post(&format!("/api/agents/{id}/input"), json!({ "text": "more output please" }));
        s.wait(&id, "idle");
    }
    let ev = s.events(&id).into_iter().find(|e| e["kind"] == "recording").unwrap();
    assert!(ev["summary"].as_str().unwrap().contains("recording stopped"), "{ev}");
    assert!(std::fs::metadata(s.data.join("sessions").join(&id).join("term.cast")).unwrap().len() <= 400);
}

#[test]
fn the_diff_holds_committed_uncommitted_and_untracked_changes() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let head = git(&r, &["rev-parse", "HEAD"]);
    let a = s.spawn(json!({ "repo": r, "name": "differ", "task": "WRITE notes.txt first note" }));
    let id = a["id"].as_str().unwrap().to_string();
    assert_eq!(a["base"], json!(head));
    s.wait(&id, "idle");
    let cwd = PathBuf::from(a["cwd"].as_str().unwrap());
    std::fs::write(cwd.join("lib.txt"), "one\ntwo\n").unwrap();
    git(&cwd, &["add", "lib.txt"]);
    git(&cwd, &["commit", "-q", "-m", "add lib"]);
    std::fs::write(cwd.join("README"), "hello\nmore\n").unwrap();

    let (status, d) = s.get(&format!("/api/agents/{id}/diff"));
    assert_eq!(status, 200, "{d}");
    assert_eq!(d["base"], json!(head));
    let files = d["files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["README", "lib.txt", "notes.txt"], "{d}");
    assert_eq!(files[0]["status"], "modified");
    let lines = &files[0]["hunks"][0]["lines"];
    assert_eq!(lines[0]["kind"], " ");
    assert_eq!(lines[1], json!({ "kind": "+", "text": "more", "new_line": 2, "old_line": null }));
    assert_eq!(files[0]["hunks"][0]["new_start"], 1);
    assert_eq!(files[1]["status"], "added");
    assert_eq!(files[1]["hunks"][0]["lines"][1]["text"], "two");
    assert_eq!(files[2]["status"], "added");
    assert_eq!(files[2]["untracked"], true);
    assert_eq!(files[2]["hunks"][0]["lines"][0], json!({ "kind": "+", "text": "first note", "new_line": 1, "old_line": null }));
    // The main checkout is untouched, and an unknown agent is not found.
    assert!(!r.join("notes.txt").exists());
    assert_eq!(s.get("/api/agents/a00000000/diff").0, 404);
}

#[test]
fn why_maps_chloectxs_blame_to_the_agent_its_lineage_and_its_rewind() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let lead = s.spawn(json!({ "repo": r, "name": "lead", "worktree": false }));
    let lead_id = lead["id"].as_str().unwrap().to_string();
    let mut m = Mcp::start(&s, &lead_id);
    let (text, err) = m.tool("spawn_agent", json!({ "task": "WRITE calc.py total = round(x)", "name": "fixer" }));
    assert!(!err, "{text}");
    let (_, v) = s.get("/api/agents");
    let child = v["agents"].as_array().unwrap().iter().find(|a| a["name"] == "fixer").unwrap().clone();
    let cid = child["id"].as_str().unwrap().to_string();
    let child = s.wait(&cid, "idle");
    let cwd = child["cwd"].as_str().unwrap().to_string();
    let sid = child["session_id"].as_str().unwrap().to_string();
    let sid8: String = sid.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let blame = json!({
        "text": "$ chloectx blame calc.py:1\n", "file": "calc.py", "line": 1, "line_text": "total = round(x)",
        "how": "the newest recorded edit that added the line",
        "edit": { "id": format!("{sid8}:5"), "session": sid, "kind": "tool_call", "tag": "Edit", "turn": 1, "seq": 5, "ts": now, "files": "calc.py", "text": "Edit calc.py: replaced\n\nwith\ntotal = round(x)" },
        "prompt": { "id": format!("{sid8}:2"), "session": sid, "kind": "prompt", "turn": 1, "seq": 2, "ts": now - 1, "text": "WRITE calc.py total = round(x)" },
        "calls": [],
        "notes": [
            { "id": format!("{sid8}:9"), "kind": "decision", "text": "Round half up for totals", "ts": now },
            { "id": format!("{sid8}:10"), "kind": "todo", "text": "check the tax", "ts": now }
        ],
        "receipts": [{ "receipt": "rc-1", "turn": 1, "source": "prompt", "rows": [
            { "row": { "id": "old0000:31", "kind": "decision", "score": 0.81, "decision": "knowledge" }, "now": ["superseded since, by old0000:58"], "text": "Round half even" },
            { "row": { "id": "web0000:3", "kind": "tool_result", "score": 0.5, "decision": "excerpt" }, "now": ["untrusted source: the web"], "text": "a page on rounding" }
        ]}],
        "causes": ["old0000:31 (decision) was in context before the edit and is superseded since, by old0000:58"],
        "edits": 1
    });
    std::fs::write(ctx.path().join("blame.json"), blame.to_string()).unwrap();

    let (status, w) = s.get(&format!("/api/why?agent={cid}&path=calc.py&line=1"));
    assert_eq!(status, 200, "{w}");
    assert_eq!(args_of(ctx.path(), "blame"), [cwd.as_str(), "blame", "calc.py:1", "--json"]);
    assert_eq!(w["found"], "chloectx", "{w}");
    assert_eq!(w["line"], json!({ "path": "calc.py", "line": 1, "text": "total = round(x)" }));
    assert_eq!(w["where"]["worktree"], json!(cwd));
    assert_eq!(w["where"]["branch"], "wb/fixer");
    assert_eq!(w["where"]["repo"], json!(r));
    assert_eq!(w["where"]["commit"], Value::Null);
    assert_eq!(w["who"]["agent"]["id"], json!(cid));
    let lineage: Vec<&str> = w["who"]["lineage"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(lineage, [lead_id.as_str()]);
    assert_eq!(w["who"]["prompt"]["text"], "WRITE calc.py total = round(x)");
    assert_eq!(w["who"]["prompt"]["by"], "lead", "{w}");
    assert_eq!(w["when"]["session"], json!(sid));
    assert_eq!(w["when"]["turn"], 1);
    let knew = w["knew"].as_array().unwrap();
    assert_eq!(knew.len(), 2);
    assert_eq!(knew[0]["now"], "superseded by old0000:58");
    assert_eq!(knew[0]["text"], "Round half even");
    assert_eq!((knew[0]["untrusted"].as_bool(), knew[1]["untrusted"].as_bool(), knew[1]["now"].as_str()), (Some(false), Some(true), Some("current")));
    assert_eq!(w["why"].as_array().unwrap().len(), 1, "a todo is not a reason: {w}");
    assert_eq!(w["why"][0]["kind"], "decision");
    assert_eq!(w["worth_a_look"].as_array().unwrap().len(), 1);
    assert!(w["missing"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("not committed")), "{w}");
    // Rewind: 20 s before the edit's tool call to 3 s after, in the recording.
    let rw = &w["rewind"];
    assert_eq!(rw["agent"], json!(cid));
    let (from, to) = (rw["from_ms"].as_i64().unwrap(), rw["to_ms"].as_i64().unwrap());
    assert!(to > from && to - from <= 23_000 && to >= 3_000, "{rw}");
    let (_, rec) = s.get(&format!("/api/agents/{cid}/recording?from_ms={from}&to_ms={to}"));
    assert!(texts(&rec).contains("EDITING calc.py: total = round(x)"), "{rec}");

    // A prompt the lead typed with send_to_agent, which ChloeCtx knows nothing of.
    let (_, err) = m.tool("send_to_agent", json!({ "id": cid, "text": "WRITE calc.py tax = 2" }));
    assert!(!err);
    s.wait(&cid, "idle");
    std::fs::write(ctx.path().join("blame.json"), json!({ "text": "", "file": "calc.py", "line": 2, "git": null, "edit": null, "edits": 0 }).to_string()).unwrap();
    let (_, w) = s.get(&format!("/api/why?agent={cid}&path=calc.py&line=2"));
    assert_eq!(w["found"], "workbench", "{w}");
    assert_eq!(w["who"]["agent"]["id"], json!(cid));
    assert_eq!(w["who"]["prompt"]["text"], "WRITE calc.py tax = 2");
    assert_eq!(w["who"]["prompt"]["by"], "lead", "{w}");
    assert!(w["missing"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("no record of an edit")), "{w}");
    assert!(w["rewind"].is_object());

    // Decisions, through `chloectx sql --json`, newest first.
    let sql = json!({
        "text": "2 rows", "columns": ["id", "ts", "session", "text", "tag", "files", "links"],
        "rows": [
            [format!("{sid8}:9"), now, sid, "Round half up for totals", "", "calc.py", []],
            ["old0000:31", now - 100, "another-session", "Round half even", "superseded", "calc.py lib.rs", [{ "label": "superseded_by", "to": format!("{sid8}:9") }]]
        ]
    });
    std::fs::write(ctx.path().join("sql.json"), sql.to_string()).unwrap();
    let (status, d) = s.get(&format!("/api/decisions?repo={}&limit=5", r.display()));
    assert_eq!(status, 200, "{d}");
    let a = args_of(ctx.path(), "sql");
    assert_eq!((a[0].as_str(), a[1].as_str(), a[2].as_str()), (r.to_str().unwrap(), "sql", "--json"));
    assert!(a[3].contains("kind = 'decision'") && a[3].ends_with("ORDER BY ts DESC LIMIT 5"), "{a:?}");
    let ds = d["decisions"].as_array().unwrap();
    assert_eq!(ds.len(), 2);
    assert_eq!(ds[0]["agent"]["id"], json!(cid));
    assert_eq!(ds[0]["superseded_by"], Value::Null);
    assert!(ds[0]["ts"].as_str().unwrap().contains('T'), "{d}");
    assert_eq!(ds[1]["agent"], Value::Null);
    assert_eq!(ds[1]["superseded_by"], json!(format!("{sid8}:9")));
    assert_eq!(ds[1]["files"], json!(["calc.py", "lib.rs"]));

    // Ask: as chloectx prints it, plus the agents of its records.
    std::fs::write(ctx.path().join("ask.json"), json!({ "text": "Totals round half up.", "ids": [format!("{sid8}:9"), "old0000:31"], "answer": "Because the lead decided so", "confidence": 0.8 }).to_string()).unwrap();
    let (status, v) = s.post("/api/ask", json!({ "repo": r, "question": "why do we round half up?" }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(args_of(ctx.path(), "ask")[1..], ["ask", "--json", "--", "why do we round half up?"]);
    assert_eq!(v["answer"], "Because the lead decided so");
    assert_eq!(v["agents"][format!("{sid8}:9")]["id"], json!(cid));
    assert!(v["agents"].get("old0000:31").is_none());
    assert_eq!(s.post("/api/ask", json!({ "repo": r, "question": " " })).0, 400);
}

#[test]
fn why_without_chloectx_answers_from_git_and_workbench() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "name": "solo", "task": "WRITE calc.py total = 1" }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    let cwd = PathBuf::from(a["cwd"].as_str().unwrap());
    git(&cwd, &["add", "calc.py"]);
    git(&cwd, &["commit", "-q", "-m", "add the total"]);
    let sha = git(&cwd, &["rev-parse", "HEAD"]);
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "WRITE calc.py tax = 2" }));
    s.wait(&id, "idle");

    let (status, w) = s.get(&format!("/api/why?agent={id}&path=calc.py&line=1"));
    assert_eq!(status, 200, "{w}");
    assert_eq!(w["found"], "workbench", "{w}");
    assert!(w["missing"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("not installed")), "{w}");
    assert_eq!(w["where"]["commit"]["sha"], json!(sha));
    assert_eq!(w["where"]["commit"]["subject"], "add the total");
    assert_eq!(w["who"]["agent"]["id"], json!(id));
    assert_eq!(w["who"]["lineage"], json!([]));
    assert_eq!(w["who"]["prompt"]["text"], "WRITE calc.py total = 1");
    assert_eq!(w["who"]["prompt"]["by"], "you");
    assert!(w["when"]["edit_ts"].is_string());
    assert!(w["rewind"]["to_ms"].as_i64().unwrap() > 0, "{w}");
    assert_eq!((w["knew"].as_array().unwrap().len(), w["why"].as_array().unwrap().len()), (0, 0));
    let (_, w) = s.get(&format!("/api/why?agent={id}&path=calc.py&line=2"));
    assert_eq!(w["who"]["prompt"]["text"], "WRITE calc.py tax = 2", "{w}");
    assert_eq!(w["who"]["prompt"]["by"], "you");
    assert_eq!(w["where"]["commit"], Value::Null);

    // A line of the main checkout that came before any agent: git alone.
    let (status, w) = s.get(&format!("/api/why?repo={}&path=README&line=1", r.display()));
    assert_eq!(status, 200, "{w}");
    assert_eq!(w["found"], "git", "{w}");
    assert_eq!(w["who"]["agent"], Value::Null);
    assert_eq!(w["where"]["commit"]["subject"], "first");
    assert_eq!(w["rewind"], Value::Null);
    // Decisions and ask say what is missing.
    let (status, d) = s.get(&format!("/api/decisions?repo={}", r.display()));
    assert_eq!(status, 200);
    assert_eq!(d["decisions"], json!([]));
    assert!(d["missing"][0].as_str().unwrap().contains("not installed"), "{d}");
    let (_, v) = s.post("/api/ask", json!({ "repo": r, "question": "why?" }));
    assert!(v["missing"][0].as_str().unwrap().contains("not installed"), "{v}");
    // Bad requests.
    assert_eq!(s.get(&format!("/api/why?agent={id}&line=1")).0, 400);
    assert_eq!(s.get(&format!("/api/why?agent={id}&path=calc.py&line=99")).0, 400);
    assert!(matches!(s.get(&format!("/api/why?agent={id}&path=../../../../etc/hostname&line=1")).0, 400 | 404));
    assert_eq!(s.get("/api/why?path=calc.py&line=1").0, 400);
    assert_eq!(http().get(format!("{}/api/why?agent={id}&path=calc.py&line=1", s.url)).call().unwrap().status().as_u16(), 401);
}
