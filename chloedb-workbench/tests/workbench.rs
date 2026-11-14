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

/// The fake Claude Code. Its hooks are the ones the settings file names;
/// each payload carries `transcript_path`, a transcript it writes as
/// Claude Code does (a JSONL line per entry, the assistant's with `usage`)
/// next to its settings, and what a hook prints is kept in `hook-out.log`
/// there.
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
SDIR=$(dirname "$SETTINGS")
TR="$SDIR/transcript.jsonl"
N=0
echo "FAKE CLAUDE sid=$SID model=$MODEL agent=$CHLOEDB_WORKBENCH_AGENT"
echo "TASK: $TASK"
echo "CWD: $(pwd)"
hook() {
  local cmd out
  cmd=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["hooks"][sys.argv[2]][0]["hooks"][0]["command"])' "$SETTINGS" "$1")
  out=$(printf '%s' "{\"transcript_path\":\"$TR\",${2#\{}" | eval "$cmd")
  [ -n "$out" ] && printf '%s\n' "$out" >> "$SDIR/hook-out.log"
  return 0
}
# A transcript entry, with its time.
tr() {
  python3 -c 'import json,sys,datetime; e=json.loads(sys.argv[2]); e["timestamp"]=datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="milliseconds").replace("+00:00","Z"); open(sys.argv[1],"a").write(json.dumps(e)+"\n")' "$TR" "$1"
}
USAGE='{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100}'
hook SessionStart '{"hook_event_name":"SessionStart","source":"startup","session_id":"'"$SID"'"}'
turn() {
  N=$((N+1))
  tr '{"type":"user","message":{"role":"user","content":"'"$1"'"}}'
  hook UserPromptSubmit '{"hook_event_name":"UserPromptSubmit","prompt":"'"$1"'","session_id":"'"$SID"'"}'
  tr '{"type":"assistant","message":{"id":"m'"$N"'a","role":"assistant","usage":'"$USAGE"',"content":[{"type":"text","text":"Working on **'"$1"'**. Then more."}]}}'
  tr '{"type":"assistant","message":{"id":"m'"$N"'a","role":"assistant","usage":'"$USAGE"',"content":[{"type":"tool_use","id":"t'"$N"'","name":"Bash","input":{"command":"echo hi"}}]}}'
  hook PreToolUse '{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"echo hi"},"session_id":"'"$SID"'"}'
  case "$1" in
    READ\ *)
      # READ <file>: the Read tool on it.
      local read='{"hook_event_name":"%s","tool_name":"Read","tool_input":{"file_path":"'"$PWD/${1#READ }"'"},"session_id":"'"$SID"'"}'
      hook PreToolUse "$(printf "$read" PreToolUse)"
      hook PostToolUse "$(printf "$read" PostToolUse)";;
    FAIL\ *)
      # FAIL <text>: a test run that fails with <text>: no PostToolUse, and
      # the error in the transcript.
      local id="tu_fail_$N"
      hook PreToolUse '{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_use_id":"'"$id"'","tool_input":{"command":"cargo test -p calc"},"session_id":"'"$SID"'"}'
      tr '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"'"$id"'","is_error":true,"content":"'"${1#FAIL }"'"}]}}';;
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
  tr '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t'"$N"'","content":"hi"}]}}'
  hook PostToolUse '{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"echo hi"},"tool_response":{"stdout":"hi"},"session_id":"'"$SID"'"}'
  tr '{"type":"assistant","message":{"id":"m'"$N"'b","role":"assistant","usage":'"$USAGE"',"content":[{"type":"text","text":"Finished '"$1"'. All good."}]}}'
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

    fn put(&self, path: &str, body: Value) -> (u16, Value) {
        let mut r = http().put(format!("{}{path}", self.url)).header("authorization", format!("Bearer {TOKEN}")).send_json(body).unwrap();
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
    assert_eq!(names, ["spawn_agent", "list_agents", "read_agent", "wait_agent", "send_to_agent", "stop_agent", "set_preview", "ask_human", "add_outcome", "update_outcome"]);

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

// The levels: narration, story, goals and outcomes, questions and rules,
// delivery, steer and the digest.

impl Server {
    fn patch(&self, path: &str, body: Value) -> (u16, Value) {
        let mut r = http().patch(format!("{}{path}", self.url)).header("authorization", format!("Bearer {TOKEN}")).send_json(body).unwrap();
        let status = r.status().as_u16();
        (status, r.body_mut().read_json().unwrap_or(Value::Null))
    }

    /// What an agent's hooks printed (the fake keeps it), one JSON a line.
    fn hook_out(&self, id: &str) -> Vec<Value> {
        let p = self.data.join("sessions").join(id).join("hook-out.log");
        std::fs::read_to_string(p).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect()
    }

    /// Polls a GET until `f` of its answer holds.
    fn get_until(&self, path: &str, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let (status, v) = self.get(path);
            if status == 200 && f(&v) {
                return v;
            }
            assert!(Instant::now() < end, "{path}: {what} never held: {v}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn contexts(outs: &[Value], event: &str) -> Vec<String> {
    outs.iter().filter(|o| o["hookSpecificOutput"]["hookEventName"] == event).map(|o| o["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string()).collect()
}

#[test]
fn narration_and_the_story_come_from_the_transcript_and_receipts() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    // The sentence of the transcript's last assistant text.
    let a = s.until(&id, "narrated", |a| a["narration"] == "Finished say hi.");
    assert!(a["narration_ts"].is_string(), "{a}");
    let sid = a["session_id"].as_str().unwrap().to_string();

    let now = now_s();
    std::fs::write(
        ctx.path().join("receipts.json"),
        json!({ "text": "", "receipts": [
            { "id": "rc-2", "ts": now, "session": sid, "turn": 1, "source": "prompt", "rows": [
                { "id": "old0000:31", "kind": "decision", "tag": "", "score": 0.91, "decision": "take", "text": "Money is i64 cents" },
                { "id": "fix0000:4", "kind": "error", "tag": "fix", "score": 0.84, "decision": "take", "text": "Batch workers must claim with a row lock" }
            ]},
            { "id": "rc-1", "ts": now - 5, "session": sid, "turn": 1, "source": "briefing", "rows": [
                { "id": "old0000:31", "kind": "decision", "tag": "", "score": 0.5, "decision": "briefing", "text": "Money is i64 cents" }
            ]}
        ]})
        .to_string(),
    )
    .unwrap();
    std::fs::write(ctx.path().join("sql.json"), json!({ "text": "", "columns": ["id", "ts", "session"], "rows": [["fix0000:4", now - 86400, "another-session"]], "count": 1 }).to_string()).unwrap();

    let (status, st) = s.get(&format!("/api/agents/{id}/story"));
    assert_eq!(status, 200, "{st}");
    assert_eq!(args_of(ctx.path(), "receipts"), [r.to_str().unwrap(), "receipts", "--session", sid.as_str(), "--limit", "500", "--json"]);
    assert_eq!(st["agent"]["id"], json!(id));
    assert_eq!(st["intent"], "say hi");
    let ch = st["chapters"].as_array().unwrap();
    assert_eq!(ch.len(), 1, "{st}");
    assert_eq!(ch[0]["turn"], 1);
    assert_eq!(ch[0]["prompt"], json!({ "text": "say hi", "by": "you" }));
    assert_eq!(ch[0]["title"], "say hi");
    assert_eq!(ch[0]["summary"], "Finished say hi. All good.");
    assert_eq!(ch[0]["state"], "done");
    assert!(ch[0]["ended_ms"].as_i64().unwrap() >= ch[0]["started_ms"].as_i64().unwrap());
    let steps = ch[0]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1, "{st}");
    assert_eq!((steps[0]["kind"].as_str(), steps[0]["label"].as_str(), steps[0]["count"].as_i64(), steps[0]["failed"].as_i64()), (Some("run"), Some("Ran echo hi"), Some(1), Some(0)));
    assert_eq!(steps[0]["events"].as_array().unwrap().len(), 2);
    // Each message's usage once: m1a (two lines) and m1b.
    assert_eq!(st["tokens"], json!({ "input": 20, "output": 10, "cache_read": 200 }));
    // What it knew, each record once, where it was first shown.
    let knew = st["knew"].as_array().unwrap();
    assert_eq!(knew.len(), 2, "{st}");
    assert_eq!((knew[0]["id"].as_str(), knew[0]["receipt"].as_str(), knew[0]["deja_vu"].as_bool()), (Some("old0000:31"), Some("rc-1"), Some(false)));
    assert_eq!((knew[1]["kind"].as_str(), knew[1]["turn"].as_i64(), knew[1]["deja_vu"].as_bool()), (Some("fix"), Some(1), Some(true)));
    let dv = &st["deja_vu"][0];
    assert_eq!(dv["record"], "fix0000:4");
    assert_eq!(dv["text"], "Batch workers must claim with a row lock");
    assert!(dv["when"].as_str().unwrap().contains('T'), "{dv}");
    assert_eq!(dv["shown_ms"], json!(now * 1000));

    // A second turn, with an edit: a second chapter, and the tokens add up.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "WRITE notes.txt hello" }));
    s.wait(&id, "idle");
    s.until(&id, "narrated again", |a| a["narration"] == "Finished WRITE notes.txt hello.");
    let (_, st) = s.get(&format!("/api/agents/{id}/story"));
    let ch = st["chapters"].as_array().unwrap();
    assert_eq!(ch.len(), 2, "{st}");
    let kinds: Vec<&str> = ch[1]["steps"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["run", "edit"], "{st}");
    assert_eq!(ch[1]["steps"][1]["label"], "Edited notes.txt");
    assert_eq!(st["tokens"], json!({ "input": 40, "output": 20, "cache_read": 400 }));
    assert_eq!(s.get("/api/agents/a00000000/story").0, 404);
}

#[test]
fn a_goal_is_planned_by_a_lead_and_outcomes_follow_their_agents() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let now = now_s();
    std::fs::write(
        ctx.path().join("sql.json"),
        json!({ "text": "", "columns": ["id", "kind", "tag", "ts", "session", "text", "files"], "rows": [
            ["abc00000:1", "decision", "", now, "s1", "Refunds key on (merchant, client_ref)", "src/refunds.rs"],
            ["abc00000:2", "error", "fix", now, "s1", "cargo test failed: claim twice; fixed with a row lock", ""]
        ], "count": 2 })
        .to_string(),
    )
    .unwrap();

    let (status, g) = s.post("/api/goals", json!({ "repo": r, "text": "Make refunds idempotent", "plan": true, "worktree": false }));
    assert_eq!(status, 201, "{g}");
    let gid = g["goal"]["id"].as_str().unwrap().to_string();
    assert!(gid.starts_with('g') && gid.len() == 9, "{g}");
    assert_eq!(g["goal"]["state"], "active");
    let lead = &g["lead"];
    let lead_id = lead["id"].as_str().unwrap().to_string();
    assert_eq!(lead["name"], "lead");
    assert_eq!(lead["goal"], json!(gid));
    let task = lead["task"].as_str().unwrap();
    assert!(task.contains("Make refunds idempotent") && task.contains("add_outcome") && task.contains("ask_human") && task.contains("spawn_agent"), "{task}");
    s.wait(&lead_id, "idle");

    // The lead adds outcomes and starts an agent for one.
    let mut m = Mcp::start(&s, &lead_id);
    let (text, err) = m.tool("add_outcome", json!({ "title": "Refunds keyed by client ref", "detail": "a retried request pays once" }));
    assert!(!err && text.starts_with("Added outcome o"), "{text}");
    let (_, os) = s.get(&format!("/api/outcomes?goal={gid}"));
    let o1 = os["outcomes"][0]["id"].as_str().unwrap().to_string();
    let (text, err) = m.tool("add_outcome", json!({ "title": "Batch refunds keyed too", "after": o1 }));
    assert!(!err, "{text}");
    let (_, os) = s.get(&format!("/api/outcomes?goal={gid}"));
    let o2 = os["outcomes"][1]["id"].as_str().unwrap().to_string();
    assert_eq!(os["outcomes"][1]["after"], json!(o1));
    assert_eq!(os["outcomes"][1]["state"], "next");
    let (text, err) = m.tool("spawn_agent", json!({ "task": "say hi", "name": "keyed", "outcome": o1 }));
    assert!(!err, "{text}");
    let (_, v) = s.get("/api/agents");
    let child = v["agents"].as_array().unwrap().iter().find(|a| a["name"] == "keyed").unwrap().clone();
    let cid = child["id"].as_str().unwrap().to_string();
    assert_eq!((child["goal"].as_str(), child["outcome"].as_str(), child["parent"].as_str()), (Some(gid.as_str()), Some(o1.as_str()), Some(lead_id.as_str())));
    s.wait(&cid, "idle");
    // It was told its outcome at its start.
    let told = contexts(&s.hook_out(&cid), "SessionStart");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains(&format!("You work on outcome {o1}")) && told[0].contains("Make refunds idempotent") && told[0].contains("update_outcome"), "{told:?}");
    assert!(s.hook_out(&lead_id).is_empty(), "the lead had nothing to be told");

    let (status, d) = s.get(&format!("/api/goals/{gid}"));
    assert_eq!(status, 200, "{d}");
    assert_eq!(d["goal"]["id"], json!(gid));
    assert_eq!((d["outcomes"][0]["state"].as_str(), d["outcomes"][0]["agent"].as_str(), d["outcomes"][0]["agent_name"].as_str()), (Some("active"), Some(cid.as_str()), Some("keyed")), "{d}");
    assert_eq!(d["outcomes"][1]["state"], "next");
    let ids: Vec<&str> = d["agents"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [lead_id.as_str(), cid.as_str()]);
    assert_eq!(d["open_questions"], 0);
    assert_eq!(d["heads_up"], json!([]));
    let learned = d["learned"].as_array().unwrap();
    assert_eq!(learned.len(), 2, "{d}");
    assert_eq!((learned[0]["kind"].as_str(), learned[1]["kind"].as_str()), (Some("decision"), Some("fix")));
    assert!(args_of(ctx.path(), "sql")[3].contains("tag = 'fix'"));
    let pulse = d["pulse"].as_array().unwrap();
    assert_eq!(pulse.len(), 60);
    assert!(pulse.iter().map(|p| p[1].as_i64().unwrap()).sum::<i64>() >= 10, "{pulse:?}");
    assert!(pulse.windows(2).all(|w| w[1][0].as_i64().unwrap() - w[0][0].as_i64().unwrap() == 60_000));

    // Marked done, it stays done after its agent ends.
    let (text, err) = m.tool("update_outcome", json!({ "id": o1, "progress": 100, "state": "done" }));
    assert!(!err && text.contains("[done]"), "{text}");
    s.post(&format!("/api/agents/{cid}/input"), json!({ "text": "exit" }));
    s.until(&cid, "exited", |a| a["running"] == false);
    // An outcome whose agent ended without finishing needs you.
    let (text, err) = m.tool("spawn_agent", json!({ "task": "say hi", "name": "batch", "outcome": o2 }));
    assert!(!err, "{text}");
    let (_, v) = s.get("/api/agents");
    let bid = v["agents"].as_array().unwrap().iter().find(|a| a["name"] == "batch").unwrap()["id"].as_str().unwrap().to_string();
    s.wait(&bid, "idle");
    s.post(&format!("/api/agents/{bid}/input"), json!({ "text": "exit" }));
    s.until(&bid, "exited", |a| a["running"] == false);
    let (_, d) = s.get(&format!("/api/goals/{gid}"));
    assert_eq!((d["outcomes"][0]["state"].as_str(), d["outcomes"][0]["progress"].as_i64()), (Some("done"), Some(100)), "{d}");
    assert_eq!(d["outcomes"][1]["state"], "needs_you", "{d}");
    let (_, list) = s.get(&format!("/api/goals?repo={}", r.display()));
    assert_eq!(list["goals"][0]["id"], json!(gid));
    assert_eq!((list["goals"][0]["outcomes_total"].as_i64(), list["goals"][0]["outcomes_done"].as_i64()), (Some(2), Some(1)));
    // The user's own outcome edits and the goal's state.
    let (status, o) = s.patch(&format!("/api/outcomes/{o2}"), json!({ "state": "dropped" }));
    assert_eq!((status, o["outcome"]["state"].as_str()), (200, Some("dropped")), "{o}");
    let (status, g) = s.patch(&format!("/api/goals/{gid}"), json!({ "state": "done" }));
    assert_eq!((status, g["goal"]["state"].as_str()), (200, Some("done")), "{g}");
    assert!(g["goal"]["done"].is_string());
    // Bad requests.
    assert_eq!(s.post("/api/agents", json!({ "repo": r, "worktree": false, "outcome": "o00000000" })).0, 400);
    assert_eq!(s.post("/api/goals", json!({ "repo": r, "text": " " })).0, 400);
    assert_eq!(s.patch(&format!("/api/outcomes/{o2}"), json!({ "state": "bogus" })).0, 400);
    assert_eq!(s.get("/api/goals/g00000000").0, 404);
    let (status, _) = s.post("/api/goals", json!({ "repo": r, "text": "no lead" }));
    assert_eq!(status, 201);
}

#[test]
fn ask_human_waits_for_the_answer_and_a_rule_is_told_and_answers_again() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    std::fs::write(ctx.path().join("ask.json"), json!({ "text": "- [decision] Totals round half up, in cents (abc00000:9)\n", "ids": ["abc00000:9"], "ms": 3 }).to_string()).unwrap();
    std::fs::write(ctx.path().join("remember.json"), "Stored decision as abc12345:7.\n").unwrap();
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "name": "asker", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.until(&id, "started", |a| a["state"] == "working");

    let question = "Round totals half up or half even?";
    let mut m = Mcp::start(&s, &id);
    let ask = json!({ "question": question, "options": [{ "label": "Round half up", "detail": "what the ledger does" }, { "label": "Round half even" }], "recommended": 0, "context": "two call sites disagree" });
    let asking = std::thread::spawn(move || {
        let out = m.tool("ask_human", ask);
        (out, m)
    });
    let v = s.get_until(&format!("/api/questions?state=open&agent={id}"), "a question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    let q = v["questions"][0].clone();
    let qid = q["id"].as_str().unwrap().to_string();
    assert_eq!((q["kind"].as_str(), q["text"].as_str(), q["recommended"].as_i64(), q["agent_name"].as_str()), (Some("ask"), Some(question), Some(0), Some("asker")), "{q}");
    assert_eq!(q["context"], "two call sites disagree");
    assert_eq!(q["repo"], json!(r));
    let a = s.agent(&id);
    assert_eq!((a["state"].as_str(), a["waiting_for"].as_str(), a["narration"].as_str()), (Some("waiting"), Some(question), Some(question)), "{a}");
    assert!(kinds(&s.events(&id)).contains(&"question".to_string()));
    // Precedent: what ChloeCtx found, and the option it speaks for.
    let v = s.get_until(&format!("/api/questions/{qid}"), "precedent", |v| v["question"]["precedent"].is_object());
    let q = &v["question"];
    assert_eq!(q["precedent"]["ids"], json!(["abc00000:9"]));
    assert_eq!((q["options"][0]["matches_precedent"].as_bool(), q["options"][1]["matches_precedent"].as_bool()), (Some(true), Some(false)), "{q}");
    assert_eq!(args_of(ctx.path(), "ask")[1..], ["ask", "--json", "--", question]);
    let (_, v) = s.get(&format!("/api/questions?repo={}", r.display()));
    assert_eq!(v["questions"].as_array().unwrap().len(), 1);
    // An open question's wait times out unanswered.
    let (_, w) = s.get(&format!("/api/questions/{qid}/wait?timeout_s=0.2"));
    assert_eq!(w["answered"], false);

    // The answer, made a rule.
    let (status, v) = s.post(&format!("/api/questions/{qid}/answer"), json!({ "option": 0, "text": "cents first", "rule": "Totals round half up" }));
    assert_eq!(status, 200, "{v}");
    assert_eq!((v["question"]["state"].as_str(), v["question"]["answered_by"].as_str(), v["question"]["answer"].as_str()), (Some("answered"), Some("you"), Some("Round half up\ncents first")));
    assert_eq!(v["rule"]["ctx_id"], "abc12345:7", "{v}");
    assert_eq!(v["rule"]["question"], json!(qid));
    assert_eq!(args_of(ctx.path(), "remember"), [r.to_str().unwrap(), "remember", "decision", "Totals round half up"]);
    let ((text, err), mut m) = asking.join().unwrap();
    assert!(!err, "{text}");
    assert!(text.starts_with("The user answered: Round half up\ncents first") && text.contains("made this a rule"), "{text}");
    assert_eq!(s.agent(&id)["state"], "working");
    assert_eq!(s.post(&format!("/api/questions/{qid}/answer"), json!({ "option": 1 })).0, 409);

    // The rule is told once, at the agent's next hook that can carry it.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "go on" }));
    s.wait(&id, "idle");
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "and again" }));
    s.wait(&id, "idle");
    let outs = s.hook_out(&id);
    let told = contexts(&outs, "UserPromptSubmit");
    assert_eq!(told.len(), 1, "{outs:?}");
    assert!(told[0].contains("Rules the user set for this repository") && told[0].contains("- Totals round half up"), "{told:?}");
    assert_eq!(outs.len(), 1, "told once: {outs:?}");
    // Another agent of the repository is told at its start.
    let b = s.spawn(json!({ "repo": r, "name": "other", "worktree": false }));
    let bid = b["id"].as_str().unwrap().to_string();
    s.until(&bid, "started", |a| a["state"] == "working");
    let end = Instant::now() + Duration::from_secs(10);
    while s.hook_out(&bid).is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(contexts(&s.hook_out(&bid), "SessionStart")[0].contains("Totals round half up"));

    // The same question again: the rule answers it at once.
    let (text, err) = m.tool("ask_human", json!({ "question": "Round totals half up or half even?", "options": ["Round half up", "Round half even"] }));
    assert!(!err, "{text}");
    let (_, rules) = s.get(&format!("/api/rules?repo={}", r.display()));
    let rid = rules["rules"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(rules["rules"][0]["active"], true);
    assert!(text.starts_with(&format!("A rule the user set ({rid}) answers it: Round half up")), "{text}");
    let (_, v) = s.get(&format!("/api/questions?agent={id}"));
    let by: Vec<&str> = v["questions"].as_array().unwrap().iter().map(|q| q["answered_by"].as_str().unwrap()).collect();
    assert!(by.contains(&format!("rule:{rid}").as_str()) && by.contains(&"you"), "{by:?}");
    // A rule turned off is told no more.
    let (status, v) = s.patch(&format!("/api/rules/{rid}"), json!({ "active": false }));
    assert_eq!((status, v["rule"]["active"].as_bool()), (200, Some(false)));
    let (_, rules) = s.get(&format!("/api/rules?repo={}", r.display()));
    assert_eq!(rules["rules"][0]["active"], false);
    // Bad requests.
    assert_eq!(s.post("/api/questions", json!({ "question": "who?" })).0, 400);
    let (status, _) = s.post("/api/questions", json!({ "agent": "a00000000", "question": "who?" }));
    assert_eq!(status, 404);
    assert_eq!(s.post(&format!("/api/questions/{qid}/answer"), json!({})).0, 409);
    let (status, v) = s.post("/api/rules", json!({ "repo": r, "text": "Never float money" }));
    assert_eq!((status, v["rule"]["active"].as_bool()), (201, Some(true)), "{v}");
}

#[test]
fn permission_prompts_are_questions_and_steers_reach_the_agent() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "task": "ASK first", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "waiting");
    let v = s.get_until(&format!("/api/questions?agent={id}&state=open"), "a permission question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    let q = &v["questions"][0];
    let qid = q["id"].as_str().unwrap().to_string();
    assert_eq!((q["kind"].as_str(), q["text"].as_str(), q["context"].as_str()), (Some("permission"), Some("Claude needs your permission to use Bash"), Some("Bash: echo hi")), "{q}");
    let labels: Vec<&str> = q["options"].as_array().unwrap().iter().map(|o| o["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Yes", "Yes, and don't ask again", "No"]);
    assert_eq!(s.agent(&id)["narration"], "Claude needs your permission to use Bash");

    // A steer while it waits rides its next PostToolUse.
    let (status, v) = s.post(&format!("/api/agents/{id}/steer"), json!({ "text": "also check the docs" }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["delivered"], "queued");
    // A permission question is answered by typing the option's number.
    let (status, v) = s.post(&format!("/api/questions/{qid}/answer"), json!({ "option": 0 }));
    assert_eq!(status, 200, "{v}");
    s.wait(&id, "idle");
    ws_read_until(&s, &id, "ANSWER: 1", &[]);
    let (_, v) = s.get(&format!("/api/questions/{qid}"));
    assert_eq!((v["question"]["state"].as_str(), v["question"]["answered_by"].as_str(), v["question"]["answer"].as_str()), (Some("answered"), Some("you"), Some("Yes")));
    let told = contexts(&s.hook_out(&id), "PostToolUse");
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("The user adds: also check the docs"), "{told:?}");

    // A steer to an idle agent is typed in: a new turn.
    let (_, v) = s.post(&format!("/api/agents/{id}/steer"), json!({ "text": "look at the README too" }));
    assert_eq!(v["delivered"], "typed", "{v}");
    ws_read_until(&s, &id, "DONE: look at the README too", &[]);
    s.wait(&id, "idle");
    assert_eq!(contexts(&s.hook_out(&id), "PostToolUse").len(), 1, "a typed steer is not told again");
    let k = kinds(&s.events(&id));
    assert_eq!(k.iter().filter(|k| *k == "steer").count(), 2, "{k:?}");

    // Answered in the terminal instead.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "ASK again" }));
    s.wait(&id, "waiting");
    let v = s.get_until(&format!("/api/questions?agent={id}&state=open"), "a second question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    let q2 = v["questions"][0]["id"].as_str().unwrap().to_string();
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "yes" }));
    s.wait(&id, "idle");
    let (_, v) = s.get(&format!("/api/questions/{q2}"));
    assert_eq!((v["question"]["state"].as_str(), v["question"]["answered_by"].as_str()), (Some("answered"), Some("terminal")), "{v}");
    // A steer to nobody, or of nothing.
    assert_eq!(s.post("/api/agents/a00000000/steer", json!({ "text": "x" })).0, 404);
    assert_eq!(s.post(&format!("/api/agents/{id}/steer"), json!({ "text": " " })).0, 400);
    // Without ChloeCtx the precedent says so.
    let (_, v) = s.get(&format!("/api/questions/{qid}"));
    assert!(v["question"]["precedent"]["missing"][0].as_str().unwrap().contains("not installed"), "{v}");
    // An exited agent's open question expires.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "ASK once more" }));
    s.wait(&id, "waiting");
    s.get_until(&format!("/api/questions?agent={id}&state=open"), "a third question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    s.post(&format!("/api/agents/{id}/stop"), json!({}));
    s.until(&id, "stopped", |a| a["running"] == false);
    let (_, v) = s.get(&format!("/api/questions?agent={id}&state=expired"));
    assert_eq!(v["questions"].as_array().unwrap().len(), 1, "{v}");
}

#[test]
fn the_digest_tells_what_happened_since_you_last_looked() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let (status, v) = s.post("/api/seen", json!({}));
    assert_eq!(status, 200, "{v}");
    let seen = v["last_seen_ms"].as_i64().unwrap();
    std::thread::sleep(Duration::from_millis(20));

    // One agent asks and is answered (and a rule is made); one is left waiting.
    let a = s.spawn(json!({ "repo": r, "name": "decided", "task": "ASK first", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "waiting");
    let v = s.get_until(&format!("/api/questions?agent={id}&state=open"), "a question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    let qid = v["questions"][0]["id"].as_str().unwrap().to_string();
    let (status, _) = s.post(&format!("/api/questions/{qid}/answer"), json!({ "option": 0, "rule": "Bash may echo" }));
    assert_eq!(status, 200);
    s.wait(&id, "idle");
    let b = s.spawn(json!({ "repo": r, "name": "stuck", "task": "ASK first", "worktree": false }));
    let bid = b["id"].as_str().unwrap().to_string();
    s.wait(&bid, "waiting");
    s.get_until(&format!("/api/questions?agent={bid}&state=open"), "a question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    let sid = s.agent(&id)["session_id"].as_str().unwrap().to_string();
    std::fs::write(
        ctx.path().join("sql.json"),
        json!({ "text": "", "columns": ["id", "kind", "tag", "ts", "session", "text", "files"], "rows": [[ "abc00000:3", "decision", "", now_s(), sid, "Echo is safe here", "" ]], "count": 1 }).to_string(),
    )
    .unwrap();

    // Since you last looked (the default).
    let (status, d) = s.get("/api/digest");
    assert_eq!(status, 200, "{d}");
    assert_eq!(d["since_ms"], json!(seen));
    assert!(d["until_ms"].as_i64().unwrap() >= seen);
    assert_eq!(d["tiles"], json!({ "done": 0, "decided": 1, "waiting": 1, "learned": 1 }), "{d}");
    assert_eq!(d["decided_by"]["you"], 1);
    assert_eq!(d["happened"], true);
    assert_eq!(d["rules"].as_array().unwrap().len(), 1);
    assert_eq!(d["rules"][0]["text"], "Bash may echo");
    assert_eq!(d["learned"][0]["agent"]["id"], json!(id), "{d}");
    let qs: Vec<&str> = d["questions"].as_array().unwrap().iter().map(|q| q["state"].as_str().unwrap()).collect();
    assert_eq!(qs, ["open", "answered"]);
    let story = d["story"].as_array().unwrap();
    assert_eq!(story.len(), 2, "{d}");
    let line_of = |aid: &str| story.iter().find(|l| l["agent"]["id"] == json!(aid)).unwrap().clone();
    let l = line_of(&id);
    assert!(l["text"].as_str().unwrap().contains("worked on “ASK first”"), "{l}");
    assert!(l["chips"].as_array().unwrap().iter().any(|c| c["kind"] == "decided" && c["text"] == "Yes"), "{l}");
    let l = line_of(&bid);
    assert!(l["text"].as_str().unwrap().contains("is waiting on you"), "{l}");
    assert!(l["chips"].as_array().unwrap().iter().any(|c| c["kind"] == "waiting"), "{l}");
    // The moments of decisions, in the agents' recordings.
    let replay = d["replay"].as_array().unwrap();
    assert_eq!(replay.len(), 2, "{d}");
    assert!(replay.iter().all(|x| x["agent"] == json!(id) && x["to_ms"].as_i64().unwrap() > x["from_ms"].as_i64().unwrap()), "{d}");
    assert!(replay.iter().any(|x| x["label"].as_str().unwrap().contains("Echo is safe here")), "{d}");

    // Seen now: nothing has happened since.
    s.post("/api/seen", json!({}));
    let (_, d) = s.get("/api/digest");
    assert_eq!(d["tiles"]["decided"], 0, "{d}");
    assert_eq!(d["tiles"]["waiting"], 1, "still waiting: {d}");
    // An explicit since.
    let (_, d) = s.get(&format!("/api/digest?since={seen}&repo={}", r.display()));
    assert_eq!(d["tiles"]["decided"], 1);
}

// The levels: Map, Preview, Flow.

fn find<'a>(list: &'a Value, key: &str, want: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|x| x.pointer(key).and_then(Value::as_str) == Some(want)).unwrap_or_else(|| panic!("no {key} = {want} in {list}"))
}

#[test]
fn the_map_shows_territory_collisions_and_pins() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    // Territory: a big file, two small ones, a deep folder.
    std::fs::create_dir_all(r.join("src/deep/er/still")).unwrap();
    std::fs::write(r.join("src/main.rs"), "fn main() {}\n".repeat(200)).unwrap();
    std::fs::write(r.join("src/a.rs"), "a\n").unwrap();
    std::fs::write(r.join("src/b.rs"), "b\n").unwrap();
    std::fs::write(r.join("src/deep/er/still/x.rs"), "x\n".repeat(10)).unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "territory"]);
    let now = now_s();
    let sql = json!({
        "text": "", "count": 5, "columns": ["id", "kind", "text", "ts", "files", "ctx", "session", "tag"],
        "rows": [
            ["s1:1", "decision", "Say hello first", now, "README", "Turn 1\nAnchor: head=abc branch=main files=README#h1 idents=hello", "other-session", ""],
            ["s1:2", "fact", "calc.py holds the totals", now, ["calc.py:1"], "", "other-session", ""],
            ["s1:3", "decision", "Say bye first", now, "README", "", "other-session", "superseded"],
            ["s1:4", "preference", "A file that is gone", now, "gone.rs", "", "other-session", ""],
            ["s1:5", "todo", "Not a pin", now, "README", "", "other-session", ""]
        ]
    });
    std::fs::write(ctx.path().join("sql.json"), sql.to_string()).unwrap();

    let one = s.spawn(json!({ "repo": r, "name": "one", "task": "WRITE calc.py x = 1" }));
    let two = s.spawn(json!({ "repo": r, "name": "two", "task": "WRITE calc.py y = 2" }));
    let (one, two) = (one["id"].as_str().unwrap().to_string(), two["id"].as_str().unwrap().to_string());
    s.wait(&one, "idle");
    s.wait(&two, "idle");
    s.post(&format!("/api/agents/{one}/input"), json!({ "text": "WRITE notes.md hello" }));
    s.wait(&one, "idle");
    s.post(&format!("/api/agents/{two}/input"), json!({ "text": "READ notes.md" }));
    s.wait(&two, "idle");

    let (status, m) = s.get(&format!("/api/map?repo={}", r.display()));
    assert_eq!(status, 200, "{m}");
    // The tree: line counts, small files folded, deep folders shut.
    let t = &m["tree"];
    assert_eq!((t["name"].as_str(), t["lines"].as_u64(), t["files"].as_u64()), (Some("proj"), Some(213), Some(5)), "{t}");
    let src = find(&t["children"], "/name", "src");
    let names: Vec<&str> = src["children"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["deep", "main.rs", "…"], "{src}");
    assert_eq!((src["children"][2]["kind"].as_str(), src["children"][2]["files"].as_u64(), src["children"][2]["lines"].as_u64()), (Some("more"), Some(2), Some(2)));
    let er = &src["children"][0]["children"][0];
    assert_eq!(er["path"], "src/deep/er");
    assert_eq!((er["children"][0]["path"].as_str(), er["children"][0]["collapsed"].as_bool(), er["children"][0]["lines"].as_u64()), (Some("src/deep/er/still"), Some(true), Some(10)));

    // Each agent's changes and reading.
    let a1 = find(&m["agents"], "/agent/id", &one);
    let paths: Vec<&str> = a1["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["calc.py", "notes.md"], "{a1}");
    assert_eq!((a1["files"][0]["added"].as_i64(), a1["files"][0]["removed"].as_i64()), (Some(1), Some(0)));
    let a2 = find(&m["agents"], "/agent/id", &two);
    assert_eq!(a2["files"].as_array().unwrap().len(), 1);
    assert_eq!(a2["reading"], json!(["notes.md"]));

    // Collisions: both changed calc.py; two read what one changed.
    let cs = m["collisions"].as_array().unwrap();
    assert_eq!(cs.len(), 2, "{m}");
    let both = find(&m["collisions"], "/kind", "both_changed");
    assert_eq!((both["path"].as_str(), both["agents"].clone()), (Some("calc.py"), json!([one, two])));
    assert_eq!(both["detail"], "one and two both changed calc.py");
    let into = find(&m["collisions"], "/kind", "heading_into");
    assert_eq!((into["path"].as_str(), into["changed_by"].as_str(), into["read_by"].as_str()), (Some("notes.md"), Some(one.as_str()), Some(two.as_str())));

    // Pins: a decision on the README at the line its anchor names, a fact
    // on a line of calc.py (which only the agents have); not the
    // superseded one, the gone file or the todo.
    let a = args_of(ctx.path(), "sql");
    assert_eq!((a[0].as_str(), a[1].as_str(), a[2].as_str()), (r.to_str().unwrap(), "sql", "--json"));
    assert!(a[3].contains("kind = 'decision'"), "{a:?}");
    let pins = m["pins"].as_array().unwrap();
    assert_eq!(pins.len(), 2, "{m}");
    let readme = find(&m["pins"], "/path", "README");
    assert_eq!((readme["line"].as_i64(), readme["record"]["id"].as_str(), readme["record"]["kind"].as_str()), (Some(1), Some("s1:1"), Some("decision")));
    assert!(readme["record"]["ts"].as_str().unwrap().contains('T'));
    assert_eq!(readme["agent"], Value::Null);
    assert_eq!(find(&m["pins"], "/path", "calc.py")["line"], 1);
    assert_eq!(m["missing"], json!([]));

    assert_eq!(s.get("/api/map").0, 400);
    assert_eq!(http().get(format!("{}/api/map?repo={}", s.url, r.display())).call().unwrap().status().as_u16(), 401);
    // Without ChloeCtx: no pins, and it says why.
    std::fs::remove_file(ctx.path().join("sql.json")).unwrap();
    let (_, m) = s.get(&format!("/api/map?repo={}", r.display()));
    assert_eq!(m["pins"], json!([]));
    assert!(m["missing"][0].as_str().unwrap().contains("chloectx sql failed"), "{m}");
}

/// A tiny app on loopback: `/` is HTML, `/old` redirects, `/form` echoes a
/// POST and sets a cookie. Keeps the head of each request it got.
fn tiny_app() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::sync::{Arc, Mutex};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut c) = conn else { continue };
            let seen = seen2.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(c.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                seen.lock().unwrap().push(head.clone());
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let html = "<html><head><link rel=stylesheet href=\"/style.css\"></head><body><h1 class=\"title\">Hello preview</h1><a href=\"/about\">about</a></body></html>";
                let (status, ctype, extra, body) = match path.as_str() {
                    "/" | "/?x=1" => ("200 OK", "text/html; charset=utf-8", String::new(), html.to_string()),
                    "/old" => ("302 Found", "text/plain", format!("Location: http://127.0.0.1:{port}/about\r\n"), String::new()),
                    "/form" => ("200 OK", "text/plain", "Set-Cookie: sid=7; Path=/; Domain=127.0.0.1\r\n".to_string(), format!("got {}", String::from_utf8_lossy(&body))),
                    _ => ("404 Not Found", "text/plain", String::new(), "no".to_string()),
                };
                let _ = write!(c, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len());
            });
        }
    });
    (port, seen)
}

/// An HTTP client that does not follow redirects.
fn http_raw() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).http_status_as_error(false).max_redirects(0).proxy(None).build().into()
}

#[test]
fn the_preview_proxies_an_agents_app_with_the_overlay() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let (port, seen) = tiny_app();
    let web = s.spawn(json!({ "repo": r, "name": "web", "task": "WRITE page.html <h1 class=title>Hello preview</h1>" }));
    let dev = s.spawn(json!({ "repo": r, "name": "dev", "task": "WRITE other.txt nothing here" }));
    let (web, dev) = (web["id"].as_str().unwrap().to_string(), dev["id"].as_str().unwrap().to_string());
    s.wait(&web, "idle");
    s.wait(&dev, "idle");

    // Only loopback http, with a port, and not Workbench's own.
    let own = s.url.rsplit(':').next().unwrap();
    for bad in ["http://example.com:80", "https://127.0.0.1:5173", "http://127.0.0.1", "http://10.0.0.1:80", "http://user@127.0.0.1:80", &format!("http://127.0.0.1:{own}")] {
        let (status, v) = s.put(&format!("/api/agents/{dev}/preview"), json!({ "url": bad }));
        assert_eq!(status, 400, "{bad}: {v}");
    }
    assert_eq!(s.put("/api/agents/a00000000/preview", json!({ "url": format!("http://127.0.0.1:{port}") })).0, 404);
    let (status, v) = s.put(&format!("/api/agents/{dev}/preview"), json!({ "url": format!("http://127.0.0.1:{port}/") }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["agent"]["preview_url"], json!(format!("http://127.0.0.1:{port}")));
    assert_eq!(s.agent(&dev)["preview_url"], json!(format!("http://127.0.0.1:{port}")));

    // The token: none, a wrong one, then the right one once, as a cookie.
    let base = format!("{}/preview/{dev}/", s.url);
    assert_eq!(http_raw().get(&base).call().unwrap().status().as_u16(), 401);
    assert_eq!(http_raw().get(format!("{base}?token=nope")).call().unwrap().status().as_u16(), 401);
    assert_eq!(http_raw().get(&base).header("cookie", "wb_preview=nope").call().unwrap().status().as_u16(), 401);
    let res = http_raw().get(format!("{base}?x=1&token={TOKEN}")).call().unwrap();
    assert_eq!(res.status().as_u16(), 303);
    assert_eq!(res.headers().get("location").unwrap(), &format!("/preview/{dev}/?x=1"));
    let cookie = res.headers().get("set-cookie").unwrap().to_str().unwrap().to_string();
    assert!(cookie.starts_with(&format!("wb_preview={TOKEN};")) && cookie.contains("Path=/preview/") && cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"), "{cookie}");
    let jar = format!("wb_preview={TOKEN}");
    // The cookie opens no /api route.
    assert_eq!(http_raw().get(format!("{}/api/agents", s.url)).header("cookie", &jar).call().unwrap().status().as_u16(), 401);

    // HTML gets the overlay and its absolute links through the proxy.
    let mut res = http_raw().get(format!("{base}?x=1")).header("cookie", &jar).call().unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.headers().get("x-frame-options").unwrap(), "SAMEORIGIN");
    let body = res.body_mut().read_to_string().unwrap();
    assert!(body.contains("<script src=\"/preview-overlay.js\"></script></body>"), "{body}");
    assert!(body.contains(&format!("href=\"/preview/{dev}/style.css\"")) && body.contains(&format!("href=\"/preview/{dev}/about\"")), "{body}");
    {
        let heads = seen.lock().unwrap();
        assert!(heads.iter().any(|h| h.starts_with("GET /?x=1 ")), "{heads:?}");
        assert!(heads.iter().all(|h| !h.contains("wb_preview") && !h.contains("token=")), "the app saw Workbench's token: {heads:?}");
    }
    // A redirect stays in the preview; a POST is passed on, its cookie kept
    // to the preview; a POST from another origin, or another host, is refused.
    let res = http_raw().get(format!("{base}old")).header("cookie", &jar).call().unwrap();
    assert_eq!(res.status().as_u16(), 302);
    assert_eq!(res.headers().get("location").unwrap(), &format!("/preview/{dev}/about"));
    let mut res = http_raw().post(format!("{base}form")).header("cookie", &jar).header("content-type", "application/x-www-form-urlencoded").header("origin", &s.url).send("a=1").unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.headers().get("set-cookie").unwrap(), &format!("sid=7; Path=/preview/{dev}/"));
    assert_eq!(res.body_mut().read_to_string().unwrap(), "got a=1");
    assert!(seen.lock().unwrap().iter().any(|h| h.starts_with("POST /form ") && h.to_ascii_lowercase().contains(&format!("origin: http://127.0.0.1:{port}"))));
    assert_eq!(http_raw().post(format!("{base}form")).header("cookie", &jar).header("origin", "http://evil.example").send("a=1").unwrap().status().as_u16(), 403);
    assert_eq!(http_raw().get(&base).header("cookie", &jar).header("host", &format!("evil.example:{own}")).call().unwrap().status().as_u16(), 403);
    // An agent with no preview, and the overlay's script.
    assert_eq!(http_raw().get(format!("{}/preview/{web}/", s.url)).header("cookie", &jar).call().unwrap().status().as_u16(), 404);
    let mut res = http_raw().get(format!("{}/preview-overlay.js", s.url)).call().unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let js = res.body_mut().read_to_string().unwrap();
    assert!(js.contains("wb-pick") && js.contains("wb-who"), "{js}");

    // A pick goes to the agent whose diff holds its text and classes,
    // with ChloeCtx's records about the files.
    let sql = json!({ "text": "", "columns": ["id", "kind", "text", "ts", "files", "session"], "rows": [
        ["s1:1", "decision", "Titles are h1", now_s(), "page.html", "x"],
        ["s1:2", "fact", "Unrelated", now_s(), "lib.rs", "x"]
    ]});
    std::fs::write(ctx.path().join("sql.json"), sql.to_string()).unwrap();
    let pick = json!({ "selector": "body > h1.title", "text": "Hello preview", "html": "<h1 class=\"title\">Hello preview</h1>", "rect": { "x": 0, "y": 0, "width": 100, "height": 20 } });
    let (status, v) = s.post("/api/preview/route", json!({ "agent": dev, "pick": pick }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["owner"]["id"], json!(web), "{v}");
    assert_eq!(v["files"], json!(["page.html"]));
    assert_eq!(v["related"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["related"][0]["id"], "s1:1");
    assert!(v["message"].as_str().unwrap().contains("body > h1.title"), "{v}");
    // Nothing matches: the preview's own agent.
    let (_, v) = s.post("/api/preview/route", json!({ "agent": dev, "pick": { "selector": "p.zzz", "text": "zzz qqq", "html": "<p class=\"zzz\">zzz qqq</p>" } }));
    assert_eq!(v["owner"]["id"], json!(dev), "{v}");
    assert_eq!(v["files"], json!([]));
    assert_eq!(s.post("/api/preview/route", json!({ "agent": dev })).0, 400);

    // An agent says where its app runs with set_preview.
    let mut m = Mcp::start(&s, &web);
    let (text, err) = m.tool("set_preview", json!({ "url": format!("http://localhost:{port}") }));
    assert!(!err, "{text}");
    assert_eq!(s.agent(&web)["preview_url"], json!(format!("http://localhost:{port}")));
    let (text, err) = m.tool("set_preview", json!({ "url": "http://example.com:80" }));
    assert!(err, "{text}");
    let (_, err) = m.tool("set_preview", json!({ "url": "" }));
    assert!(!err);
    assert_eq!(s.agent(&web)["preview_url"], Value::Null);
}

#[test]
fn flow_shows_where_time_went_and_applies_an_allow() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let report = json!({ "project": "proj", "days": 1.0, "suggestions": [{ "kind": "slow_command", "text": "cargo test took 5m 10s over 5 runs", "seconds": 310 }] });
    std::fs::write(ctx.path().join("report.json"), report.to_string()).unwrap();
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "name": "busy", "worktree": false, "task": "ASK first" }));
    let id = a["id"].as_str().unwrap().to_string();
    // Approved three times; failed three times with one error; one file
    // read four times.
    for i in 0..3 {
        if i > 0 {
            s.post(&format!("/api/agents/{id}/input"), json!({ "text": "ASK again" }));
        }
        s.wait(&id, "waiting");
        s.post(&format!("/api/agents/{id}/input"), json!({ "text": "yes" }));
        s.wait(&id, "idle");
    }
    for _ in 0..3 {
        s.post(&format!("/api/agents/{id}/input"), json!({ "text": "FAIL error: sqlx offline data missing" }));
        s.wait(&id, "idle");
    }
    for _ in 0..4 {
        s.post(&format!("/api/agents/{id}/input"), json!({ "text": "READ src.txt" }));
        s.wait(&id, "idle");
    }

    let (status, f) = s.get(&format!("/api/flow?repo={}&minutes=60", r.display()));
    assert_eq!(status, 200, "{f}");
    let w = &f["window"];
    assert_eq!(w["to_ms"].as_i64().unwrap() - w["from_ms"].as_i64().unwrap(), 3_600_000);
    let ag = &f["agents"][0];
    assert_eq!(ag["agent"]["id"], json!(id));
    let kinds: std::collections::BTreeSet<&str> = ag["segments"].as_array().unwrap().iter().map(|s| s[2].as_str().unwrap()).collect();
    for k in ["model", "tools", "tests", "waiting", "redo", "idle"] {
        assert!(kinds.contains(k), "no {k} segment: {ag}");
        assert!(ag[format!("{k}_ms")].as_i64().unwrap() > 0, "{k}: {ag}");
        assert_eq!(ag[format!("{k}_ms")], f["totals"][format!("{k}_ms")]);
    }
    let segs = ag["segments"].as_array().unwrap();
    assert!(segs.windows(2).all(|p| p[0][1].as_i64() <= p[1][0].as_i64()), "{ag}");
    // Ten turns of the fake's usage, two assistant messages a turn.
    assert_eq!(f["tokens"], json!({ "input": 200, "output": 100, "cache_read": 2000 }));
    assert_eq!(ag["tokens"], f["tokens"]);

    // The coach.
    let allow = find(&f["suggestions"], "/kind", "allow");
    assert_eq!(allow["id"], "allow:Bash(echo hi:*)");
    assert_eq!(allow["action"], json!({ "kind": "allow", "pattern": "Bash(echo hi:*)" }));
    assert_eq!(allow["applied"], false);
    assert!(allow["detail"].as_str().unwrap().contains("approved it 3 times"), "{allow}");
    let fix = find(&f["suggestions"], "/kind", "fix");
    assert_eq!(fix["action"]["error"], "error: sqlx offline data missing");
    assert!(fix["title"].as_str().unwrap().contains("3 times"), "{fix}");
    let capsule = find(&f["suggestions"], "/kind", "capsule");
    assert_eq!(capsule["action"]["files"], json!(["src.txt"]));
    let from_report = find(&f["suggestions"], "/from", "chloectx");
    assert_eq!((from_report["id"].as_str(), from_report["title"].as_str(), from_report["saves_ms"].as_i64()), (Some("report:0"), Some("cargo test took 5m 10s over 5 runs"), Some(310_000)));
    assert_eq!(f["report"], report);
    assert_eq!(args_of(ctx.path(), "report"), [r.to_str().unwrap(), "report", "--json", "--days", "1"]);

    // Apply: the pattern is kept, and agents started from then on have it.
    let (status, v) = s.post("/api/flow/apply", json!({ "id": "allow:Bash(echo hi:*)", "repo": r }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["allowed"], json!(["Bash(echo hi:*)"]));
    let (_, f2) = s.get(&format!("/api/flow?repo={}", r.display()));
    assert_eq!(find(&f2["suggestions"], "/kind", "allow")["applied"], true);
    let b = s.spawn(json!({ "repo": r, "name": "later", "worktree": false }));
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(s.data.join("sessions").join(b["id"].as_str().unwrap()).join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["permissions"]["allow"], json!(["Bash(echo hi:*)"]));
    assert!(settings["hooks"]["Stop"].is_array());
    let first: Value = serde_json::from_str(&std::fs::read_to_string(s.data.join("sessions").join(&id).join("settings.json")).unwrap()).unwrap();
    assert!(first.get("permissions").is_none(), "{first}");

    // A fix is remembered through ChloeCtx; the others say how and apply nothing.
    let fix_id = fix["id"].as_str().unwrap();
    assert_eq!(s.post("/api/flow/apply", json!({ "id": fix_id, "repo": r })).0, 400);
    std::fs::write(ctx.path().join("remember.json"), "Stored fact as s1:9.").unwrap();
    let (status, v) = s.post("/api/flow/apply", json!({ "id": fix_id, "repo": r, "text": "run cargo sqlx prepare" }));
    assert_eq!(status, 200, "{v}");
    assert_eq!(args_of(ctx.path(), "remember")[1..], ["remember", "fact", "When this error comes back: “error: sqlx offline data missing”, the fix is: run cargo sqlx prepare"]);
    let (_, f3) = s.get(&format!("/api/flow?repo={}", r.display()));
    assert_eq!(find(&f3["suggestions"], "/kind", "fix")["applied"], true);
    assert_eq!(s.post("/api/flow/apply", json!({ "id": capsule["id"], "repo": r })).0, 400);
    assert_eq!(s.get("/api/flow").0, 400);
}
