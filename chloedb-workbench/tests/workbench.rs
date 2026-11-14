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
        // Sessions without ChloeCtx unless the test asks for it with
        // "+chloectx" (ctxwire: ChloeCtx is required outside tests).
        let with_ctx = args.contains(&"+chloectx");
        let args: Vec<&str> = args.iter().copied().filter(|a| *a != "+chloectx").chain((!with_ctx).then_some("--no-chloectx")).collect();
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
    assert_eq!(names, ["spawn_agent", "list_agents", "read_agent", "wait_agent", "send_to_agent", "stop_agent", "set_preview", "ask_human", "add_outcome", "update_outcome", "list_incidents", "test_history", "blast_radius", "prepare_pr", "propose_change", "search_code", "search_docs", "test_with_persona", "runtime_errors", "trace", "hotspots"]);

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

// ChloeCtx is required: each session gets its hooks and MCP server.

#[test]
fn sessions_get_chloectx_and_without_it_the_setup_check_says_how() {
    // Missing: the check says so with the fix, and an agent is refused.
    let s = Server::start(&["+chloectx", "--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    let (_, st) = s.get("/api/chloectx");
    assert_eq!(st["required"], true, "{st}");
    assert_eq!(st["ok"], false, "{st}");
    assert!(st["fix"].as_str().unwrap().contains("install.sh --workbench"), "{st}");
    let (status, v) = s.post("/api/agents", json!({ "repo": r, "task": "say hi" }));
    assert_eq!(status, 409, "{v}");
    assert_eq!(v["error_code"], "chloectx_missing", "{v}");
    assert!(!git(&r, &["worktree", "list"]).contains("worktrees"), "no worktree is left behind");
    drop(s);

    // There: its wiring is merged into the session's files, run for the
    // session's own folder.
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    std::fs::write(
        ctx.path().join("wiring.json"),
        json!({
            "wired": null,
            "settings": {
                "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "/opt/chloectx hook Stop", "timeout": 30}]}],
                          "PostCompact": [{"hooks": [{"type": "command", "command": "/opt/chloectx hook PostCompact", "timeout": 15}]}]},
                "permissions": {"allow": ["mcp__chloectx"]},
                "env": {"CHLOECTX_DISTILL_MODEL": "haiku"}
            },
            "mcp": {"chloectx": {"type": "stdio", "command": "/opt/chloectx", "args": ["mcp"]}},
            "model": {"ok": true}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(ctx.path().join("--version.json"), "chloectx 0.3.2\n").unwrap();
    let s = Server::start(&["+chloectx", "--chloectx-bin", bin.to_str().unwrap()]);
    let (_, st) = s.get("/api/chloectx");
    assert_eq!(st["ok"], true, "{st}");
    let a = s.spawn(json!({ "repo": r, "task": "say hi", "name": "ctx" }));
    let id = a["id"].as_str().unwrap();
    let sess = s.data.join("sessions").join(id);
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(sess.join("settings.json")).unwrap()).unwrap();
    let stop: Vec<&str> = settings["hooks"]["Stop"].as_array().unwrap().iter().map(|e| e["hooks"][0]["command"].as_str().unwrap()).collect();
    assert_eq!(stop.len(), 2, "{settings}");
    assert!(stop[0].ends_with(" hook") && stop[1] == "/opt/chloectx hook Stop", "{stop:?}");
    assert_eq!(settings["hooks"]["PostCompact"][0]["hooks"][0]["command"], "/opt/chloectx hook PostCompact");
    assert_eq!(settings["env"]["CHLOECTX_DISTILL_MODEL"], "haiku");
    assert!(settings["permissions"]["allow"].as_array().unwrap().contains(&json!("mcp__chloectx")), "{settings}");
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(sess.join("mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["chloectx"]["command"], "/opt/chloectx");
    assert!(mcp["mcpServers"]["workbench"].is_object());
    // Asked for the worktree the session runs in.
    let args = args_of(ctx.path(), "wiring");
    assert_eq!(args[0], a["cwd"].as_str().unwrap(), "{args:?}");
    assert!(args.iter().any(|x| x == "--project") && args.iter().any(|x| x == a["cwd"].as_str().unwrap()), "{args:?}");
}

// Fourth build: Health (SPEC.md, rows 4 and 5): test runs from the Bash
// hooks and the radar, alert rules as SQL, incidents and their actions.

/// Posts a hook's JSON as the agent's hook command would.
fn hook_as(s: &Server, id: &str, body: Value) {
    let mut r = http().post(format!("{}/api/hook", s.url)).header("authorization", format!("Bearer {TOKEN}")).header("x-workbench-agent", id).send_json(body).unwrap();
    let status = r.status().as_u16();
    assert_eq!(status, 200, "{}", r.body_mut().read_to_string().unwrap_or_default());
}

/// A Bash call of `command` that printed `out`: PreToolUse, then
/// PostToolUse (or PostToolUseFailure with exit code 101).
fn bash_as(s: &Server, id: &str, use_id: &str, command: &str, out: &str, failed: bool) {
    hook_as(s, id, json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": use_id, "tool_input": { "command": command } }));
    if failed {
        hook_as(s, id, json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_use_id": use_id, "tool_input": { "command": command }, "error": format!("Exit code 101\n{out}") }));
    } else {
        hook_as(s, id, json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": use_id, "tool_input": { "command": command }, "tool_response": { "stdout": out, "stderr": "", "interrupted": false } }));
    }
}

fn cargo_out(coupon_ok: bool) -> String {
    let coupon = if coupon_ok { "ok" } else { "FAILED" };
    let tail = if coupon_ok {
        "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s".to_string()
    } else {
        "failures:\n\n---- totals::coupon stdout ----\nthread 'totals::coupon' panicked at tests/totals.rs:88:5:\nassertion `left == right` failed\n  left: 1799\n right: 1800\n\nfailures:\n    totals::coupon\n\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s".to_string()
    };
    format!("running 2 tests\ntest totals::rounds ... ok\ntest totals::coupon ... {coupon}\n\n{tail}\n")
}

#[test]
fn health_records_test_runs_and_the_radar_finds_flaky_and_newly_failing() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "name": "cart", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    s.until(&id, "started", |a| a["session_id"].is_string());
    let radar = format!("/api/tests?repo={}", r.display());
    // Passed, then failed on the same commit with the same tree: flaky,
    // and newly failing.
    bash_as(&s, &id, "tu1", "cargo test totals", &cargo_out(true), false);
    s.get_until(&radar, "the first run", |v| v["totals"]["commands"] == 1);
    bash_as(&s, &id, "tu2", "cargo test totals", &cargo_out(false), true);
    let v = s.get_until(&radar, "the second run", |v| v["totals"]["commands"] == 2);
    assert_eq!(v["totals"]["tests"], 2, "{v}");
    assert_eq!(v["totals"]["runs"], 4, "{v}");
    let coupon = find(&v["tests"], "/test", "totals::coupon");
    assert_eq!((coupon["runner"].as_str(), coupon["runs"].as_i64(), coupon["passed"].as_i64(), coupon["failed"].as_i64()), (Some("cargo"), Some(2), Some(1), Some(1)), "{coupon}");
    assert_eq!(coupon["flaky"], true, "{coupon}");
    assert!(coupon["flaky_why"].as_str().unwrap().contains("same code"), "{coupon}");
    assert_eq!(coupon["newly_failing"], true, "{coupon}");
    assert_eq!(coupon["last"]["outcome"], "failed");
    assert!(coupon["p50_ms"].as_i64().is_some(), "{coupon}");
    let rounds = find(&v["tests"], "/test", "totals::rounds");
    assert_eq!((rounds["pass_rate"].as_f64(), rounds["flaky"].as_bool()), (Some(1.0), Some(false)));
    assert_eq!(v["totals"]["flaky"], 1);
    assert_eq!(v["totals"]["newly_failing"], 1);
    assert!(matches!(v["percentiles"].as_str(), Some("approx_percentile" | "rust")), "{v}");
    let cmds = v["commands"].as_array().unwrap();
    assert_eq!((cmds[0]["exit_code"].as_i64(), cmds[0]["failed"].as_i64(), cmds[0]["passed"].as_i64()), (Some(101), Some(1), Some(1)), "{v}");
    assert_eq!(cmds[1]["exit_code"], 0);

    // One test's runs, newest first, with its error.
    let (status, h) = s.get(&format!("/api/tests/history?repo={}&test=totals::coupon", r.display()));
    assert_eq!(status, 200, "{h}");
    let runs = h["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0]["outcome"].as_str(), runs[1]["outcome"].as_str()), (Some("failed"), Some("passed")));
    assert!(runs[0]["error"].as_str().unwrap().contains("left: 1799"), "{h}");
    assert_eq!(runs[0]["commit"], runs[1]["commit"]);
    assert_eq!(s.get(&format!("/api/tests/history?repo={}&test=nosuch", r.display())).0, 404);

    // The agent's runs, for Story's chip: each at the seq of its PreToolUse.
    let (_, ar) = s.get(&format!("/api/tests/runs?agent={id}"));
    let runs = ar["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2, "{ar}");
    assert_eq!(runs[0]["failed_tests"], json!(["totals::coupon"]));
    assert_eq!(runs[0]["newly_failing"], json!(["totals::coupon"]));
    let evs = s.events(&id);
    let pre = evs.iter().find(|e| e["kind"] == "PreToolUse" && e["data"].as_str().is_some_and(|d| d.contains("\"tu2\""))).unwrap();
    assert_eq!(runs[0]["seq"], pre["seq"]);

    // A runner it does not parse: one row for the command, with its exit code.
    bash_as(&s, &id, "tu3", "make check", "all good\n", false);
    let v = s.get_until(&radar, "make check", |v| v["totals"]["commands"] == 3);
    let mk = find(&v["tests"], "/test", "make check");
    assert_eq!((mk["runner"].as_str(), mk["passed"].as_i64()), (Some("other"), Some(1)), "{mk}");

    // A failed run with no PostToolUse (Claude Code fires none then) is
    // read from the transcript when the turn ends.
    s.post(&format!("/api/agents/{id}/input"), json!({ "text": "FAIL error: could not compile calc" }));
    s.wait(&id, "idle");
    let v = s.get_until(&radar, "the failed run from the transcript", |v| v["totals"]["commands"] == 4);
    let calc = find(&v["tests"], "/test", "cargo test -p calc");
    assert_eq!(calc["last"]["outcome"], "failed", "{calc}");

    // Other agents' MCP: the radar and one test's history as text.
    let mut m = Mcp::start(&s, &id);
    m.send("initialize", json!({}));
    m.notify("notifications/initialized");
    let (text, err) = m.tool("test_history", json!({}));
    assert!(!err, "{text}");
    assert!(text.contains("totals::coupon") && text.contains("FLAKY") && text.contains("NEWLY FAILING"), "{text}");
    let (text, err) = m.tool("test_history", json!({ "test": "totals::coupon" }));
    assert!(!err && text.contains("failed") && text.contains("by cart"), "{text}");
    assert_eq!(s.get("/api/tests/runs").0, 400);
}

#[test]
fn health_rules_open_an_incident_with_a_board_and_act_on_it() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    let cart = s.spawn(json!({ "repo": r, "name": "cart", "worktree": false }))["id"].as_str().unwrap().to_string();
    let checkout = s.spawn(json!({ "repo": r, "name": "checkout", "worktree": false }))["id"].as_str().unwrap().to_string();
    s.until(&cart, "started", |a| a["session_id"].is_string());
    s.until(&checkout, "started", |a| a["session_id"].is_string());

    // The built-in rules, each of whose SQL runs.
    let (status, rs) = s.get("/api/alerts/rules");
    assert_eq!(status, 200, "{rs}");
    let ids: Vec<&str> = rs["rules"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["test-refail", "tokens-hour", "same-file", "idle-asked"], "{rs}");
    for rule in rs["rules"].as_array().unwrap() {
        let (status, t) = s.post("/api/alerts/test", json!({ "sql": rule["sql"], "cmp": rule["cmp"], "threshold": rule["threshold"], "window_s": rule["window_s"] }));
        assert_eq!(status, 200);
        assert_eq!(t["ok"], true, "{}: {t}", rule["id"]);
        assert_eq!(t["columns"][0], "entity", "{t}");
    }
    let (_, t) = s.post("/api/alerts/test", json!({ "sql": "DELETE FROM agents" }));
    assert_eq!(t["ok"], false, "{t}");
    let (_, t) = s.post("/api/alerts/test", json!({ "sql": "SELECT x FROM nosuch" }));
    assert_eq!(t["ok"], false, "{t}");
    assert!(t["error"].is_string(), "{t}");

    // Two agents edit one file: an incident, and Needs you.
    for (who, n) in [(&cart, 2), (&checkout, 1)] {
        for _ in 0..n {
            hook_as(&s, who, json!({ "hook_event_name": "PostToolUse", "tool_name": "Edit", "tool_input": { "file_path": r.join("src/totals.rs"), "old_string": "a", "new_string": "b" }, "tool_response": {} }));
        }
    }
    let (status, c) = s.post("/api/alerts/check", json!({}));
    assert_eq!(status, 200, "{c}");
    assert_eq!(c["counts"]["open"], 1, "{c}");
    let inc = &c["incidents"][0];
    let iid = inc["id"].as_str().unwrap().to_string();
    assert_eq!(inc["title"], "cart and checkout both edit src/totals.rs", "{inc}");
    assert_eq!(inc["files"], json!(["src/totals.rs"]));
    assert_eq!(inc["needs_you"], true);
    assert_eq!(inc["firings"][0]["rule"], "same-file");
    assert!(inc["firings"][0]["summary"].as_str().unwrap().contains("cart (2 edits), checkout (1 edit)"), "{inc}");
    assert_eq!(inc["actions"][0]["kind"], "notify");
    // Run again: the same firing, no second incident.
    let (_, c) = s.post("/api/alerts/check", json!({}));
    assert_eq!((c["counts"]["open"].as_i64(), c["started"].as_i64()), (Some(1), Some(0)), "{c}");

    // cart's test fails five times: that joins the incident (it shares cart)
    // and asks you in Decide.
    let radar = format!("/api/tests?repo={}", r.display());
    for i in 0..5 {
        bash_as(&s, &cart, &format!("tf{i}"), "cargo test totals", &cargo_out(false), true);
        s.get_until(&radar, "the run", |v| v["totals"]["commands"] == i + 1);
    }
    let (_, c) = s.post("/api/alerts/check", json!({}));
    assert_eq!(c["counts"]["open"], 1, "{c}");
    let inc = &c["incidents"][0];
    assert_eq!(inc["rules"], json!(["same-file", "test-refail"]), "{inc}");
    assert_eq!(inc["severity"], "major");
    let ask = inc["actions"].as_array().unwrap().iter().find(|a| a["kind"] == "ask").unwrap_or_else(|| panic!("no ask: {inc}")).clone();
    assert_eq!(ask["state"], "open");
    let qid = ask["ref"].as_str().unwrap().to_string();
    let (_, qs) = s.get("/api/questions?state=open");
    let q = find(&qs["questions"], "/id", &qid);
    assert_eq!(q["kind"], "alert");
    assert_eq!(q["agent"], json!(cart));
    assert!(q["text"].as_str().unwrap().contains("totals::coupon failed 5 times"), "{q}");
    assert_eq!(q["options"][0]["label"], "Pause cart");

    // The board.
    let (status, b) = s.get(&format!("/api/incidents/{iid}"));
    assert_eq!(status, 200, "{b}");
    let board = &b["board"];
    assert_eq!(board["firings"].as_array().unwrap().len(), 2, "{b}");
    assert_eq!(board["tests"][0]["test"], "totals::coupon", "{b}");
    assert!(board["error"].as_str().unwrap().contains("left: 1799"), "{b}");
    assert_eq!(board["sentences"].as_array().unwrap().len(), 2);
    let kinds: Vec<&str> = board["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"fired") && kinds.contains(&"PostToolUseFailure"), "{kinds:?}");
    assert!(board["missing"][0].as_str().unwrap().contains("ChloeCtx is not installed"), "{b}");
    assert_eq!(s.get("/api/incidents/i00000000").0, 404);

    // Answering "Pause cart" sends Esc to cart.
    let (status, v) = s.post(&format!("/api/questions/{qid}/answer"), json!({ "option": 0 }));
    assert_eq!(status, 200, "{v}");
    let l = s.get_until("/api/incidents?state=open", "the pause", |v| v["incidents"][0]["actions"].as_array().is_some_and(|a| a.iter().any(|x| x["kind"] == "pause")));
    let acts = l["incidents"][0]["actions"].as_array().unwrap();
    let pause = acts.iter().find(|a| a["kind"] == "pause").unwrap();
    assert_eq!((pause["state"].as_str(), pause["detail"].as_str()), (Some("done"), Some("Paused cart (Esc)")), "{l}");
    assert_eq!(acts.iter().find(|a| a["kind"] == "ask").unwrap()["state"], "answered");
    // And by hand: notify, or a kind it does not know.
    let (status, v) = s.post(&format!("/api/incidents/{iid}/act"), json!({ "kind": "notify", "agent": checkout }));
    assert_eq!((status, v["action"]["kind"].as_str()), (200, Some("notify")), "{v}");
    assert_eq!(s.post(&format!("/api/incidents/{iid}/act"), json!({ "kind": "reboot" })).0, 400);

    // MCP's list_incidents.
    let mut m = Mcp::start(&s, &checkout);
    m.send("initialize", json!({}));
    m.notify("notifications/initialized");
    let (text, err) = m.tool("list_incidents", json!({}));
    assert!(!err && text.contains(&iid) && text.contains("both edit src/totals.rs") && text.contains("▲ A test re-run failing 5 times"), "{text}");

    // Resolved by you: its firings are silenced, so the next run opens nothing.
    let (status, v) = s.post(&format!("/api/incidents/{iid}/resolve"), json!({}));
    assert_eq!((status, v["incident"]["state"].as_str()), (200, Some("resolved")), "{v}");
    let (_, c) = s.post("/api/alerts/check", json!({}));
    assert_eq!((c["counts"]["open"].as_i64(), c["started"].as_i64()), (Some(0), Some(0)), "{c}");

    // Rules are rows: made, changed, switched off, deleted (a built-in is not).
    let body = json!({ "name": "Many edits", "sql": "SELECT agent AS entity, agent, COUNT(*) AS v FROM file_edits WHERE ts >= :since GROUP BY agent", "cmp": ">=", "threshold": 2, "window_s": 600, "action": "none" });
    let (status, v) = s.post("/api/alerts/rules", body);
    assert_eq!(status, 201, "{v}");
    let rid = v["rule"]["id"].as_str().unwrap().to_string();
    assert_eq!(s.post("/api/alerts/rules", json!({ "name": "bad", "sql": "SELECT FROM" })).0, 400);
    let (_, c) = s.post("/api/alerts/check", json!({}));
    assert_eq!(c["counts"]["open"], 1, "{c}");
    assert_eq!(c["incidents"][0]["rules"], json!([rid]), "{c}");
    let (status, v) = s.patch(&format!("/api/alerts/rules/{rid}"), json!({ "enabled": false, "threshold": 50 }));
    assert_eq!((status, v["rule"]["enabled"].as_bool(), v["rule"]["threshold"].as_f64()), (200, Some(false), Some(50.0)), "{v}");
    let (_, c) = s.post("/api/alerts/check", json!({}));
    assert_eq!(c["counts"]["open"], 0, "{c}");
    assert_eq!(s.patch(&format!("/api/alerts/rules/{rid}"), json!({ "cmp": "~" })).0, 400);
    let del = |id: &str| http().delete(format!("{}/api/alerts/rules/{id}", s.url)).header("authorization", format!("Bearer {TOKEN}")).call().unwrap().status().as_u16();
    assert_eq!(del("same-file"), 409);
    assert_eq!(del(&rid), 200);
    assert_eq!(del(&rid), 404);
}

// Fourth build: Ship (use cases 7 and 11).

/// `fake_chloectx` with the two commands Ship needs to act: `pack OUT`
/// writes a capsule (`OUT/manifest.json`) and prints `pack.json`, and
/// `mount DIR` answers MCP over stdio, its `tools/call` with `mount.json`
/// (the call it was sent is kept in `mount.call`).
fn fake_ship_chloectx(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-chloectx");
    let script = format!(
        r#"#!/bin/bash
D='{}'
printf '%s\n' "$PWD" "$@" > "$D/$1.args"
case "$1" in
  pack)
    [ -f "$D/pack.json" ] || {{ echo "chloectx: no store for $PWD" >&2; exit 1; }}
    mkdir -p "$2" && cp "$D/pack.json" "$2/manifest.json" && cat "$D/pack.json"; exit 0;;
  mount)
    [ -f "$2/manifest.json" ] || {{ echo "chloectx mount: no capsule in $2" >&2; exit 1; }}
    while read -r line; do
      case "$line" in
        *tools/call*) printf '%s\n' "$line" > "$D/mount.call"; cat "$D/mount.json"; echo;;
        *initialize\"*) echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-06-18","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"capsule","version":"0"}}}}}}';;
      esac
    done
    exit 0;;
esac
[ -f "$D/$1.json" ] || {{ echo "chloectx: no store for $PWD" >&2; exit 1; }}
cat "$D/$1.json"
"#,
        dir.display()
    );
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A repository "shop": `cartTotal` in src/cart.ts, called from
/// src/checkout.ts and named by tests/cart.test.ts.
fn shop() -> (tempfile::TempDir, PathBuf) {
    let (d, r) = repo();
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::create_dir_all(r.join("tests")).unwrap();
    std::fs::write(r.join("src/cart.ts"), "export function cartTotal(items) {\n  let sum = 0;\n  for (const i of items) sum += i.price * i.qty;\n  return sum;\n}\n").unwrap();
    std::fs::write(r.join("src/checkout.ts"), "import { cartTotal } from \"./cart\";\n\nexport function createSession(cart) {\n  return { total: cartTotal(cart.items) };\n}\n").unwrap();
    std::fs::write(r.join("tests/cart.test.ts"), "import { cartTotal } from \"../src/cart\";\n\ntest(\"sums the lines\", () => {\n  expect(cartTotal([{ price: 2, qty: 3 }])).toBe(6);\n});\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "shop"]);
    (d, r)
}

/// The agent's change in its worktree: `cartTotal` changed, `taxFor` new.
fn charge_tax(cwd: &Path) {
    std::fs::write(
        cwd.join("src/cart.ts"),
        "export function cartTotal(items) {\n  let sum = 0;\n  for (const i of items) sum += i.price * i.qty;\n  return sum + taxFor(sum);\n}\n\nexport function taxFor(sum) {\n  return Math.round(sum * 0.08);\n}\n",
    )
    .unwrap();
}

#[test]
fn blast_radius_names_functions_tests_decisions_and_agents_and_the_pr_carries_a_capsule() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_ship_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = shop();
    let a = s.spawn(json!({ "repo": r, "name": "checkout-tax", "task": "Charge tax by address. Then the rest." }));
    let id = a["id"].as_str().unwrap().to_string();
    let a = s.wait(&id, "idle");
    let cwd = PathBuf::from(a["cwd"].as_str().unwrap());
    let sid = a["session_id"].as_str().unwrap().to_string();
    let sid8: String = sid.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    charge_tax(&cwd);
    // Another agent in the same file.
    let o = s.spawn(json!({ "repo": r, "name": "cart-ui", "task": "WRITE src/cart.ts // badge" }));
    let oid = o["id"].as_str().unwrap().to_string();
    s.wait(&oid, "idle");

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let mine = format!("{sid8}:9");
    std::fs::write(ctx.path().join("blame.json"), json!({ "file": "src/cart.ts", "line": 4, "edit": { "session": sid }, "notes": [
        { "id": mine, "kind": "decision", "text": "Tax is added to the cart total, rounded per order", "ts": now },
        { "id": format!("{sid8}:10"), "kind": "todo", "text": "check the rounding", "ts": now }
    ] }).to_string()).unwrap();
    std::fs::write(ctx.path().join("sql.json"), json!({ "text": "2 rows", "columns": ["id", "kind", "text", "ts", "files", "ctx", "session", "tag", "links"], "rows": [
        [mine, "decision", "Tax is added to the cart total, rounded per order", now, "src/cart.ts", "", sid, "", []],
        ["old00000:31", "decision", "No tax in cartTotal", now - 100, "src/cart.ts", "", "another-session", "superseded", []]
    ] }).to_string()).unwrap();
    std::fs::write(ctx.path().join("rpc.json"), json!({ "nodes": [
        { "id": "gr000000:4", "type": "note", "kind": "preference", "label": "Money in cents", "text": "Money is kept in cents", "ts": now, "current": true },
        { "id": "gr000000:7", "type": "note", "kind": "decision", "label": "Rates by region", "text": "Tax rates come from the region table", "ts": now, "current": true },
        { "id": "e:tax", "type": "entity", "label": "tax" },
        { "id": "f:src/cart.ts", "type": "file", "label": "src/cart.ts" },
        { "id": "f:src/rates.ts", "type": "file", "label": "src/rates.ts" }
    ], "edges": [
        { "s": "gr000000:4", "t": "f:src/cart.ts", "label": "touches" },
        { "s": "gr000000:4", "t": "e:tax", "label": "about" },
        { "s": "gr000000:7", "t": "e:tax", "label": "about" },
        { "s": "gr000000:7", "t": "f:src/rates.ts", "label": "touches" }
    ] }).to_string()).unwrap();

    let (status, b) = s.get(&format!("/api/agents/{id}/blast"));
    assert_eq!(status, 200, "{b}");
    assert_eq!(b["agent"]["id"], json!(id));
    assert_eq!(b["base"], json!(a["base"]));
    let f = |name: &str| b["functions"].as_array().unwrap().iter().find(|f| f["name"] == name).cloned().unwrap_or_else(|| panic!("no {name}: {b}"));
    let total = f("cartTotal");
    assert_eq!((total["status"].as_str(), total["path"].as_str(), total["line"].as_u64(), total["lang"].as_str()), (Some("changed"), Some("src/cart.ts"), Some(1), Some("typescript")), "{total}");
    assert_eq!(total["callers"][0]["path"], "src/checkout.ts", "{total}");
    assert_eq!(total["tests"], json!(["tests/cart.test.ts"]));
    assert_eq!(f("taxFor")["status"], "added");
    assert_eq!(b["totals"]["functions"], 2);
    let t = &b["tests"][0];
    assert_eq!(t["path"], "tests/cart.test.ts");
    assert!(t["covers"].as_array().unwrap().contains(&json!("src/cart.ts#cartTotal")), "{t}");
    // Health's test_runs table is there, with no runs of this test yet.
    assert!(t["runs"].is_null() || t["runs"] == json!(0) || t["runs"] == json!([]), "no runs yet: {t}");
    assert_eq!(b["sources"]["test_runs"], true, "{b}");
    // Decisions: blame (a todo is no decision), the file's records, the graph and its one hop.
    let d = |did: &str| b["decisions"].as_array().unwrap().iter().find(|d| d["id"] == did).cloned().unwrap_or_else(|| panic!("no {did}: {b}"));
    let dm = d(&mine);
    assert!(dm["via"].as_array().unwrap().contains(&json!("blame")) && dm["via"].as_array().unwrap().contains(&json!("file")), "{dm}");
    assert_eq!(dm["agent"]["id"], json!(id));
    assert_eq!(dm["line"], json!({ "path": "src/cart.ts", "line": 4 }));
    assert_eq!(d("old00000:31")["superseded"], true);
    assert_eq!(d("gr000000:4")["via"], json!(["graph"]));
    assert_eq!(d("gr000000:7")["entity"], "tax");
    assert!(b["decisions"].as_array().unwrap().iter().all(|d| d["kind"] != "todo"));
    assert_eq!(b["related_files"][0]["path"], "src/rates.ts", "{b}");
    assert_eq!(b["sources"]["graph"], true);
    let ba = args_of(ctx.path(), "blame");
    assert_eq!(&ba[1..], ["blame", ba[2].as_str(), "--json", "--no-replay"]);
    assert!(ba[2].starts_with("src/cart.ts:"), "{ba:?}");
    // The other agent, in the same file.
    let g = &b["agents"][0];
    assert_eq!(g["agent"]["id"], json!(oid), "{b}");
    assert!(g["kinds"].as_array().unwrap().contains(&json!("same_file")), "{g}");
    let risk = &b["risk"];
    assert!(matches!(risk["level"].as_str(), Some("medium" | "high")), "{risk}");
    assert_eq!(risk["line"], "Medium risk: 2 functions changed with 2 callers; cart-ui changes src/cart.ts too.");
    assert!(risk["reasons"].as_array().unwrap().iter().any(|r| r.as_str().unwrap().contains("old00000:31")), "{risk}");

    // Asked before there is a capsule: the repository's memory.
    std::fs::write(ctx.path().join("ask.json"), json!({ "text": format!("Tax is added per order [{mine}]."), "ids": [mine] }).to_string()).unwrap();
    let (status, v) = s.post(&format!("/api/agents/{id}/capsule/ask"), json!({ "question": "why per order?" }));
    assert_eq!(status, 200, "{v}");
    assert_eq!((v["available"].as_bool(), v["via"].as_str()), (Some(true), Some("repo")), "{v}");
    assert_eq!(args_of(ctx.path(), "ask"), [cwd.to_str().unwrap(), "ask", "--json", "--", "why per order?"]);
    assert_eq!(v["agents"][&mine]["id"], json!(id));
    assert!(v["missing"][0].as_str().unwrap().contains("no capsule"), "{v}");
    assert_eq!(s.post(&format!("/api/agents/{id}/capsule/ask"), json!({ "question": " " })).0, 400);

    // The draft, before the capsule.
    let (status, p) = s.get(&format!("/api/agents/{id}/pr"));
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["title"], "Charge tax by address");
    assert_eq!((p["branch"].as_str(), p["base_branch"].as_str()), (Some("wb/checkout-tax"), Some("main")));
    let body = p["body"].as_str().unwrap();
    for h in ["## Summary", "## Decisions behind it", "## Blast radius", "## Tests run", "## Ask why"] {
        assert!(body.contains(h), "no {h} in {body}");
    }
    assert!(body.contains(&format!("◆ `{mine}`")) && body.contains("`taxFor` (new)") && body.contains("No capsule was packed"), "{body}");
    assert_eq!(p["sections"]["decisions"][0]["id"], json!(mine), "its session's decision first: {p}");
    let file = PathBuf::from(p["body_file"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), body);
    assert!(p["gh"].as_str().unwrap().starts_with("gh pr create --base main --head wb/checkout-tax --title \"Charge tax by address\" --body-file "), "{p}");
    assert_eq!(p["push"], "git push -u origin wb/checkout-tax");
    assert_eq!(s.get(&format!("/api/agents/{id}/capsule")).1["capsule"], Value::Null);

    // Packed: since the agent started, into its session directory.
    std::fs::write(ctx.path().join("pack.json"), json!({ "format": 1, "title": "checkout-tax: Charge tax by address", "records": 12, "by_kind": { "decision": 3, "prompt": 9 }, "bytes": 40960, "sha256": "9b1e0c47d2a3", "created_ts": now, "since_ts": now - 120, "total_ms": 812.0 }).to_string()).unwrap();
    let (status, c) = s.post(&format!("/api/agents/{id}/capsule"), json!({}));
    assert_eq!(status, 200, "{c}");
    assert_eq!(c["available"], true, "{c}");
    let cap = &c["capsule"];
    let dir = cap["dir"].as_str().unwrap().to_string();
    assert!(Path::new(&dir).join("manifest.json").is_file());
    assert!(dir.ends_with("/capsule") && dir.contains(&id), "{dir}");
    assert_eq!((cap["records"].as_u64(), cap["sha256"].as_str()), (Some(12), Some("9b1e0c47d2a3")));
    assert_eq!(cap["mount"], json!(format!("chloectx mount {dir}")));
    let pa = args_of(ctx.path(), "pack");
    assert_eq!(pa[0], cwd.to_str().unwrap());
    assert_eq!((pa[1].as_str(), pa[2].as_str(), pa[3].as_str()), ("pack", dir.as_str(), "--since-days"));
    let days: f64 = pa[4].parse().unwrap();
    assert!(days > 0.0 && days < 0.01, "{pa:?}");
    assert_eq!(&pa[5..], ["--title", "checkout-tax: Charge tax by address", "--json"]);
    assert_eq!(s.get(&format!("/api/agents/{id}/capsule")).1["capsule"]["dir"], json!(dir));

    // Asked again: the capsule answers, through `chloectx mount`.
    std::fs::write(ctx.path().join("mount.json"), json!({ "jsonrpc": "2.0", "id": 2, "result": { "content": [{ "type": "text", "text": format!("- [{mine}] decision: tax per order.\n\n(capsule \"checkout-tax\" sha256:9b1e0c47d2a3, records: {mine}, gr000000:4)") }] } }).to_string()).unwrap();
    let (_, v) = s.post(&format!("/api/agents/{id}/capsule/ask"), json!({ "question": "why per order?" }));
    assert_eq!((v["available"].as_bool(), v["via"].as_str()), (Some(true), Some("capsule")), "{v}");
    assert_eq!(v["ids"], json!([mine, "gr000000:4"]));
    let call: Value = serde_json::from_str(&std::fs::read_to_string(ctx.path().join("mount.call")).unwrap()).unwrap();
    assert_eq!((call["params"]["name"].as_str(), call["params"]["arguments"]["question"].as_str()), (Some("capsule_ask"), Some("why per order?")));
    assert_eq!(args_of(ctx.path(), "mount")[1..], ["mount".to_string(), dir.clone()]);

    // The draft says how to ask it.
    let (_, p) = s.get(&format!("/api/agents/{id}/pr"));
    let body = p["body"].as_str().unwrap();
    assert!(body.contains(&format!("chloectx mount {dir}")) && body.contains("12 records"), "{body}");
    assert_eq!(p["capsule"]["dir"], json!(dir));

    // The agent's own tools: blast_radius, prepare_pr.
    let mut m = Mcp::start(&s, &id);
    let (text, err) = m.tool("blast_radius", json!({}));
    assert!(!err, "{text}");
    assert!(text.contains("cartTotal") && text.contains("tests/cart.test.ts") && text.contains(&mine), "{text}");
    let (text, err) = m.tool("prepare_pr", json!({ "capsule": false }));
    assert!(!err, "{text}");
    assert!(text.contains("Title: Charge tax by address") && text.contains("gh pr create") && text.contains("## Ask why"), "{text}");
    assert_eq!(s.get("/api/agents/a00000000/blast").0, 404);
}

#[test]
fn ship_without_chloectx_still_says_the_blast_radius_and_drafts_the_pr() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = shop();
    let a = s.spawn(json!({ "repo": r, "name": "solo", "task": "Charge tax" }));
    let id = a["id"].as_str().unwrap().to_string();
    let a = s.wait(&id, "idle");
    charge_tax(Path::new(a["cwd"].as_str().unwrap()));
    let (status, b) = s.get(&format!("/api/agents/{id}/blast"));
    assert_eq!(status, 200, "{b}");
    assert_eq!(b["totals"]["functions"], 2, "{b}");
    assert_eq!(b["decisions"], json!([]));
    assert_eq!((b["sources"]["chloectx"].as_bool(), b["sources"]["graph"].as_bool()), (Some(false), Some(false)));
    assert!(b["missing"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("not installed")), "{b}");
    let (_, c) = s.post(&format!("/api/agents/{id}/capsule"), json!({}));
    assert_eq!(c["available"], false, "{c}");
    assert!(c["why"].as_str().unwrap().contains("not installed"), "{c}");
    let (_, v) = s.post(&format!("/api/agents/{id}/capsule/ask"), json!({ "question": "why?" }));
    assert_eq!(v["available"], false, "{v}");
    let (status, p) = s.get(&format!("/api/agents/{id}/pr"));
    assert_eq!(status, 200, "{p}");
    assert!(p["body"].as_str().unwrap().contains("ChloeCtx holds no decision"), "{p}");
    assert!(p["gh"].as_str().unwrap().contains("--head wb/solo"));
}

// Fourth build: Time and guard (SPEC.md, rows 8, 9 and 6).

/// A request with another token, as an agent's hooks and MCP server send.
fn call_as(s: &Server, method: &str, path: &str, token: &str, agent: Option<&str>, body: Option<Value>) -> (u16, Value) {
    let url = format!("{}{path}", s.url);
    let auth = format!("Bearer {token}");
    let mut r = match (method, body) {
        ("GET", _) => http().get(&url).header("authorization", &auth).call().unwrap(),
        ("PUT", b) => http().put(&url).header("authorization", &auth).send_json(b.unwrap_or(json!({}))).unwrap(),
        (_, b) => {
            let req = http().post(&url).header("authorization", &auth);
            let req = match agent {
                Some(a) => req.header("x-workbench-agent", a),
                None => req,
            };
            req.send_json(b.unwrap_or(json!({}))).unwrap()
        }
    };
    let status = r.status().as_u16();
    (status, r.body_mut().read_json().unwrap_or(Value::Null))
}

/// The token an agent was started with (its MCP config's).
fn agent_token(s: &Server, id: &str) -> String {
    let mcp: Value = serde_json::from_str(&std::fs::read_to_string(s.data.join("sessions").join(id).join("mcp.json")).unwrap()).unwrap();
    mcp["mcpServers"]["workbench"]["env"]["CHLOEDB_WORKBENCH_TOKEN"].as_str().unwrap().to_string()
}

fn pre_bash(s: &Server, token: &str, id: &str, cmd: &str) -> Value {
    let (status, v) = call_as(s, "POST", "/api/hook", token, Some(id), Some(json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": cmd }, "session_id": "s" })));
    assert_eq!(status, 200, "{v}");
    v["output"].clone()
}

#[test]
fn guarded_commands_are_denied_proposed_and_decided_only_by_the_user() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let a = s.spawn(json!({ "repo": r, "name": "mig", "task": "say hi" }));
    let id = a["id"].as_str().unwrap().to_string();
    let cwd = PathBuf::from(a["cwd"].as_str().unwrap());
    s.wait(&id, "idle");
    // Its key is its own, and opens the API.
    let key = agent_token(&s, &id);
    assert_ne!(key, TOKEN);
    assert_eq!(call_as(&s, "GET", "/api/agents", &key, None, None).0, 200);
    assert_eq!(call_as(&s, "GET", "/api/agents", "not-a-token", None, None).0, 401);

    // The guarded list: the built-ins, on.
    let (status, g) = s.get(&format!("/api/guarded?repo={}", r.display()));
    assert_eq!(status, 200, "{g}");
    let ids: Vec<&str> = g["rules"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["migrations", "deploy", "push-default", "rm-outside"]);
    assert!(g["rules"][2]["detail"].as_str().unwrap().contains("main"), "{g}");

    // PreToolUse: a guarded Bash command is denied, pointing to propose_change.
    let out = pre_bash(&s, &key, &id, "npx prisma migrate deploy");
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(out["hookSpecificOutput"]["permissionDecisionReason"].as_str().unwrap().contains("propose_change"), "{out}");
    for cmd in ["git push origin main", "rm -rf /srv/elsewhere", "cd / && rm -r srv", "./scripts/deploy.sh"] {
        assert_eq!(pre_bash(&s, &key, &id, cmd)["hookSpecificOutput"]["permissionDecision"], "deny", "{cmd}");
    }
    assert_eq!(pre_bash(&s, &key, &id, "cargo test && rm -rf target"), Value::Null);
    assert!(s.events(&id).iter().any(|e| e["kind"] == "guarded" && e["summary"].as_str().unwrap().contains("prisma migrate deploy")));

    // The list is the user's to change: a prefix added, migrations off.
    let mut rules = g["rules"].as_array().unwrap().clone();
    rules[0]["on"] = json!(false);
    rules.push(json!({ "kind": "prefix", "label": "make seed", "on": true }));
    assert_eq!(call_as(&s, "PUT", "/api/guarded", &key, None, Some(json!({ "repo": r, "rules": rules }))).0, 403);
    let (status, g2) = s.put("/api/guarded", json!({ "repo": r, "rules": rules }));
    assert_eq!(status, 200, "{g2}");
    assert_eq!(g2["rules"][0]["on"], false);
    assert_eq!(g2["rules"][4]["label"], "make seed");
    assert_eq!(pre_bash(&s, &key, &id, "npx prisma migrate deploy"), Value::Null);
    assert_eq!(pre_bash(&s, &key, &id, "make seed")["hookSpecificOutput"]["permissionDecision"], "deny");
    s.put("/api/guarded", json!({ "repo": r, "rules": g["rules"] }));

    // A proposal, from the agent's key: a row with its rule and risk.
    let (status, v) = call_as(&s, "POST", "/api/changes", &key, None, Some(json!({ "command": "npx prisma migrate deploy", "why": "the saved_at column" })));
    assert_eq!(status, 201, "{v}");
    let c1 = v["request"].clone();
    assert_eq!((c1["state"].as_str(), c1["rule"].as_str(), c1["risk"].as_str(), c1["agent"].as_str()), (Some("open"), Some("migrations"), Some("medium"), Some(id.as_str())));
    assert_eq!(c1["rule_label"], "Database migrations");
    assert_eq!(c1["agent_name"], "mig");

    // Only the user declines; the agent is told why.
    let c1id = c1["id"].as_str().unwrap();
    assert_eq!(call_as(&s, "POST", &format!("/api/changes/{c1id}/decline"), &key, None, Some(json!({}))).0, 403);
    let (status, v) = s.post(&format!("/api/changes/{c1id}/decline"), json!({ "reason": "not before the backup" }));
    assert_eq!(status, 200, "{v}");
    // It is working (its hooks said so): told at its next hook, or typed
    // when its turn ends.
    assert_eq!((v["request"]["state"].as_str(), v["request"]["told"].as_str()), (Some("declined"), Some("queued")));
    call_as(&s, "POST", "/api/hook", &key, Some(&id), Some(json!({ "hook_event_name": "Stop", "session_id": "s" })));
    let screen = ws_read_until(&s, &id, "not before the backup", &[]);
    assert!(screen.contains("declined your change request"), "{screen}");
    s.wait(&id, "idle");

    // Approve runs it in the agent's worktree, keeps its output and tells it.
    let (status, v) = call_as(&s, "POST", "/api/changes", &key, None, Some(json!({ "command": "echo applied | tee applied.txt", "why": "a test" })));
    assert_eq!(status, 201, "{v}");
    let c2id = v["request"]["id"].as_str().unwrap().to_string();
    assert_eq!(v["request"]["risk"], "low");
    let (status, v) = call_as(&s, "POST", &format!("/api/changes/{c2id}/approve"), &key, None, Some(json!({})));
    assert_eq!(status, 403, "{v}");
    assert!(v["error"].as_str().unwrap().contains("only the user"), "{v}");
    let (status, v) = s.post(&format!("/api/changes/{c2id}/approve"), json!({}));
    assert_eq!(status, 200, "{v}");
    let c2 = &v["request"];
    assert_eq!((c2["state"].as_str(), c2["exit_code"].as_i64(), c2["decided_by"].as_str()), (Some("done"), Some(0), Some("user")), "{c2}");
    assert_eq!(c2["output"].as_str().unwrap().trim(), "applied");
    assert_eq!(std::fs::read_to_string(cwd.join("applied.txt")).unwrap(), "applied\n");
    assert_eq!(c2["told"], "typed");
    ws_read_until(&s, &id, "succeeded (exit 0)", &[]);
    assert_eq!(s.post(&format!("/api/changes/{c2id}/approve"), json!({})).0, 409);

    // A failing one fails, and says so.
    let (_, v) = call_as(&s, "POST", "/api/changes", &key, None, Some(json!({ "command": "echo nope >&2; exit 4" })));
    let c3id = v["request"]["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    let (_, v) = s.post(&format!("/api/changes/{c3id}/approve"), json!({}));
    assert_eq!((v["request"]["state"].as_str(), v["request"]["exit_code"].as_i64()), (Some("failed"), Some(4)), "{v}");
    assert_eq!(v["request"]["output"].as_str().unwrap().trim(), "nope");

    // The list, newest first; by state.
    let (_, l) = s.get(&format!("/api/changes?repo={}", r.display()));
    let states: Vec<&str> = l["requests"].as_array().unwrap().iter().map(|x| x["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["failed", "done", "declined"]);
    let (_, l) = s.get("/api/changes?state=declined");
    assert_eq!(l["requests"].as_array().unwrap().len(), 1);
    // A request comes from an agent.
    assert_eq!(s.post("/api/changes", json!({ "command": "x" })).0, 400);
}

#[test]
fn time_travel_answers_mission_and_map_as_they_were() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    let (status, t) = s.get(&format!("/api/time?repo={}", r.display()));
    assert_eq!(status, 200, "{t}");
    assert_eq!((t["keep_ms"].as_i64(), t["every_ms"].as_i64()), (Some(86_400_000), Some(600_000)));
    assert_eq!(t["snapshots"], json!([]));
    assert_eq!(s.get(&format!("/api/time?repo={}&at={}", r.display(), t["now_ms"])).1["available"], false);

    // A goal set is a snapshot.
    let (status, v) = s.post("/api/goals", json!({ "repo": r, "text": "Make refunds idempotent", "worktree": false }));
    assert_eq!(status, 201, "{v}");
    let gid = v["goal"]["id"].as_str().unwrap().to_string();
    let t = s.get_until(&format!("/api/time?repo={}", r.display()), "a goal snapshot", |t| t["snapshots"].as_array().is_some_and(|l| l.iter().any(|k| k["reason"] == "goal")));
    let k1 = t["snapshots"].as_array().unwrap().iter().find(|k| k["reason"] == "goal").unwrap().clone();
    assert_eq!(k1["detail"], "goal set: Make refunds idempotent");
    assert!(s.data.join("snapshots").join(k1["id"].as_str().unwrap()).is_dir());

    // An agent's uncommitted work, kept by a manual snapshot.
    let w = s.spawn(json!({ "repo": r, "name": "notes", "task": "WRITE notes.txt hello" }));
    let wid = w["id"].as_str().unwrap().to_string();
    s.wait(&wid, "idle");
    let wcwd = PathBuf::from(w["cwd"].as_str().unwrap());
    let head = git(&wcwd, &["rev-parse", "HEAD"]);
    // Events per minute, for the scrubber.
    let t = s.get(&format!("/api/time?repo={}", r.display())).1;
    assert!(t["events"].as_array().unwrap().iter().map(|e| e[1].as_i64().unwrap()).sum::<i64>() > 0, "{t}");
    let (status, v) = s.post("/api/time/snapshot", json!({}));
    assert_eq!(status, 201, "{v}");
    let k2 = v["snapshot"].clone();
    assert_eq!(k2["reason"], "manual");
    let at2 = k2["ts_ms"].as_i64().unwrap();
    // Then the goal is done and the file is gone.
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(s.patch(&format!("/api/goals/{gid}"), json!({ "state": "done" })).0, 200);
    std::fs::remove_file(wcwd.join("notes.txt")).unwrap();
    s.get_until(&format!("/api/time?repo={}", r.display()), "a second goal snapshot", |t| t["snapshots"].as_array().is_some_and(|l| l.iter().any(|k| k["detail"] == "goal done: Make refunds idempotent")));
    assert_eq!(s.get(&format!("/api/goals/{gid}")).1["goal"]["state"], "done");

    // As of the manual snapshot: the goal was active, the file was there.
    let (status, p) = s.get(&format!("/api/time?repo={}&at={at2}", r.display()));
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["available"], true);
    assert_eq!(p["snapshot"]["id"], k2["id"]);
    assert_eq!(p["distance_ms"], 0);
    assert_eq!(p["mission"]["goals"][0]["id"], json!(gid));
    assert_eq!(p["mission"]["goals"][0]["state"], "active");
    assert_eq!(p["mission"]["detail"]["goal"]["state"], "active");
    assert_eq!(p["mission"]["details"][&gid]["goal"]["text"], "Make refunds idempotent");
    assert!(p["agents"].as_array().unwrap().iter().any(|a| a["id"] == json!(wid)));
    let hd = p["heads"].as_array().unwrap().iter().find(|h| h["agent"] == json!(wid)).unwrap_or_else(|| panic!("{p}")).clone();
    assert_eq!((hd["head"].as_str(), hd["dirty"].as_bool(), hd["files"].as_i64()), (Some(head.as_str()), Some(true), Some(1)), "{hd}");
    // Its uncommitted changes, as a commit on its HEAD.
    let tree = hd["tree"].as_str().unwrap();
    assert_ne!(tree, head);
    assert_eq!(git(&wcwd, &["show", &format!("{tree}:notes.txt")]), "hello");
    assert_eq!(git(&wcwd, &["rev-parse", &format!("{tree}^")]), head);
    let m = &p["map"];
    let notes = m["agents"].as_array().unwrap().iter().find(|x| x["agent"]["id"] == json!(wid)).unwrap();
    assert_eq!(notes["files"][0]["path"], "notes.txt");
    // Live, the file is gone.
    let (_, live) = s.get(&format!("/api/map?repo={}", r.display()));
    let now_files = live["agents"].as_array().unwrap().iter().find(|x| x["agent"]["id"] == json!(wid)).unwrap()["files"].clone();
    assert_eq!(now_files, json!([]));
    // Long before any snapshot: the nearest, the first.
    let (_, p0) = s.get(&format!("/api/time?repo={}&at={}", r.display(), k1["ts_ms"].as_i64().unwrap() - 3_600_000));
    assert_eq!(p0["snapshot"]["id"], k1["id"]);
    assert_eq!(s.get(&format!("/api/time?repo={}&at=yesterday", r.display())).0, 400);
}

#[test]
fn routing_suggests_the_model_similar_tasks_did_best_on() {
    let s = Server::start(&[]);
    let (_d, r) = repo();
    // Past tasks: five like it on haiku, finished; one other on opus.
    let tasks = [
        ("fix the flaky cart sync test", "haiku"),
        ("fix the flaky cart total test", "haiku"),
        ("fix flaky cart sync retry test", "haiku"),
        ("fix the cart sync test timeout", "haiku"),
        ("make the flaky cart test pass", "haiku"),
        ("design the refunds service schema", "opus"),
    ];
    for (i, (task, model)) in tasks.iter().enumerate() {
        let a = s.spawn(json!({ "repo": r, "name": format!("past-{i}"), "task": task, "model": model, "worktree": false }));
        let id = a["id"].as_str().unwrap().to_string();
        s.wait(&id, "idle");
        s.post(&format!("/api/agents/{id}/input"), json!({ "text": "exit" }));
        s.wait(&id, "exited");
    }
    let q = |task: &str| format!("/api/route?repo={}&task={}", r.display(), task.replace(' ', "%20"));
    let v = s.get_until(&q("fix the flaky cart sync test again"), "five similar tasks", |v| v["similar"].as_u64().unwrap_or(0) >= 5);
    assert_eq!(v["apply"], true, "{v}");
    assert_eq!(v["suggestion"]["model"], "haiku", "{v}");
    assert_eq!((v["suggestion"]["n"].as_u64(), v["suggestion"]["done"].as_u64()), (Some(5), Some(5)), "{v}");
    assert!(v["suggestion"]["avg_tokens"].as_i64().unwrap() > 0, "{v}");
    assert!(v["why"].as_str().unwrap().contains("Haiku"), "{v}");
    assert_eq!(v["examples"][0]["model"], "haiku");
    assert!(v["examples"][0]["task"].as_str().unwrap().contains("cart"));
    // Too few words, another repository's tasks, nothing alike.
    let (_, few) = s.get(&q("fix it"));
    assert_eq!((few["similar"].as_u64(), few["suggestion"].is_null()), (Some(0), true));
    let (_d2, r2) = repo();
    let (_, other) = s.get(&format!("/api/route?repo={}&task=fix%20the%20flaky%20cart%20sync%20test", r2.display()));
    assert_eq!((other["similar"].as_u64(), other["apply"].as_bool()), (Some(0), Some(false)), "{other}");
    let (_, unlike) = s.get(&q("write the quarterly billing report"));
    assert_eq!(unlike["apply"], false, "{unlike}");

    // spawn_agent without a model takes the suggestion; with one, its own.
    let lead = s.spawn(json!({ "repo": r, "name": "lead", "worktree": false }));
    let mut m = Mcp::start(&s, lead["id"].as_str().unwrap());
    m.send("initialize", json!({}));
    let (text, err) = m.tool("spawn_agent", json!({ "task": "fix the flaky cart sync test once more", "name": "routed", "worktree": false }));
    assert!(!err, "{text}");
    let routed = s.get("/api/agents").1["agents"].as_array().unwrap().iter().find(|a| a["name"] == "routed").unwrap().clone();
    assert_eq!(routed["model"], "haiku");
    let (text, err) = m.tool("spawn_agent", json!({ "task": "write the quarterly billing report", "name": "plain", "worktree": false }));
    assert!(!err, "{text}");
    let plain = s.get("/api/agents").1["agents"].as_array().unwrap().iter().find(|a| a["name"] == "plain").unwrap().clone();
    assert_eq!(plain["model"], Value::Null);
}

// Fourth build: Search (code search for agents, docs on tap).

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A repository with a Rust file, a TypeScript one, notes and a manifest.
fn shop_repo() -> (tempfile::TempDir, PathBuf) {
    let (d, r) = repo();
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::create_dir_all(r.join("web")).unwrap();
    std::fs::write(r.join("src/cart.rs"), "use std::fmt;\n\n/// Sums the cart's lines, in cents.\npub fn cart_total(lines: &[i64]) -> i64 {\n    lines.iter().sum()\n}\n\n/// Rounds a discount half up.\npub fn round_discount(cents: i64) -> i64 {\n    (cents + 50) / 100 * 100\n}\n").unwrap();
    std::fs::write(r.join("web/useCart.ts"), "import { useMemo } from \"react\";\n\nexport const cartTotal = (lines: number[]) => {\n  return lines.reduce((a, b) => a + b, 0);\n};\n\nexport function CartSummary() {\n  return null;\n}\n").unwrap();
    std::fs::write(r.join("NOTES.md"), "# Shop\n\nMoney is integer cents.\nPrices include tax.\nCarts expire after a day.\n\n## Checkout\n\nEvery payment carries an idempotency key.\n").unwrap();
    std::fs::write(r.join("Cargo.toml"), "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n").unwrap();
    std::fs::write(r.join("Cargo.lock"), "[[package]]\nname = \"serde\"\nversion = \"1.0.214\"\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "shop"]);
    (d, r)
}

fn paths(v: &Value) -> Vec<String> {
    v["hits"].as_array().unwrap().iter().map(|h| h["path"].as_str().unwrap().to_string()).collect()
}

#[test]
fn code_search_finds_identifiers_both_ways_and_packs_them() {
    let s = Server::start(&[]);
    let (_d, r) = shop_repo();
    let repo = enc(r.to_str().unwrap());
    let (st, v) = s.get(&format!("/api/search/code?repo={repo}&q=cartTotal"));
    assert_eq!(st, 200, "{v}");
    let p = paths(&v);
    assert!(p.contains(&"src/cart.rs".to_string()) && p.contains(&"web/useCart.ts".to_string()), "camelCase finds snake_case: {v}");
    let h = v["hits"].as_array().unwrap().iter().find(|h| h["path"] == "src/cart.rs").unwrap();
    assert_eq!(h["symbol"], "cart_total", "{h}");
    assert_eq!(h["kind"], "fn");
    assert_eq!(h["start_line"], 1, "a short head joins the first definition: {h}");
    assert_eq!(h["end_line"], 6);
    assert!(h["text"].as_str().unwrap().contains("/// Sums the cart's lines"), "{h}");
    assert_eq!(v["index"]["state"], "ready", "{v}");
    assert!(v["index"]["files"].as_i64().unwrap() >= 5, "{v}");

    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=cart_total"));
    assert!(paths(&v).contains(&"web/useCart.ts".to_string()), "snake_case finds camelCase: {v}");
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=idempotency"));
    assert_eq!(paths(&v), ["NOTES.md"], "{v}");
    assert_eq!(v["hits"][0]["symbol"], "Checkout");
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=zebra"));
    assert_eq!(v["hits"], json!([]), "{v}");
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=cartTotal&lang=typescript"));
    assert!(paths(&v).iter().all(|p| p == "web/useCart.ts"), "{v}");
    assert_eq!(v["hits"][0]["symbol"], "cartTotal", "{v}");

    // PACK: one text in the budget, each chunk under its path and lines.
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=cart%20total&tokens=200"));
    let pack = &v["pack"];
    assert!(pack["used"].as_i64().unwrap() > 0 && pack["used"].as_i64().unwrap() <= 200, "{pack}");
    assert_eq!(pack["budget"], 200);
    let text = pack["text"].as_str().unwrap();
    assert!(text.contains("// src/cart.rs:1-6 fn cart_total") || text.contains("// web/useCart.ts:"), "{text}");
    assert!(pack["rows"].as_array().unwrap().iter().all(|r| r["decision"].is_string()), "{pack}");

    // The index, and a full pass that sees a new file and a removed one.
    let (_, v) = s.get(&format!("/api/search/index?repo={repo}"));
    assert!(v["index"]["langs"].as_array().unwrap().iter().any(|l| l["lang"] == "rust"), "{v}");
    std::fs::write(r.join("src/shipping.rs"), "pub fn shipping_fee(weight_grams: i64) -> i64 {\n    weight_grams / 10\n}\n").unwrap();
    let (st, v) = s.post("/api/search/reindex", json!({ "repo": r }));
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["changed"], 1, "{v}");
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=shippingFee"));
    assert_eq!(paths(&v), ["src/shipping.rs"], "{v}");
    std::fs::remove_file(r.join("src/shipping.rs")).unwrap();
    let (_, v) = s.post("/api/search/reindex", json!({ "repo": r }));
    assert_eq!(v["removed"], 1, "{v}");
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=shippingFee"));
    assert_eq!(v["hits"], json!([]), "{v}");
    let (st, _) = s.get("/api/search/code?repo=relative/path&q=x");
    assert_eq!(st, 400);
}

#[test]
fn an_agents_edit_is_reindexed_in_its_worktree() {
    let s = Server::start(&[]);
    let (_d, r) = shop_repo();
    let repo = enc(r.to_str().unwrap());
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=coupon"));
    assert_eq!(v["hits"], json!([]), "{v}");
    let a = s.spawn(json!({ "repo": r, "name": "cart", "task": "WRITE src/cart.rs pub fn apply_coupon_code() {}" }));
    let id = a["id"].as_str().unwrap().to_string();
    s.wait(&id, "idle");
    // As the agent sees it: its worktree's version of the file.
    let v = s.get_until(&format!("/api/search/code?repo={repo}&q=applyCouponCode&agent={id}"), "the edit is indexed", |v| !v["hits"].as_array().unwrap().is_empty());
    let h = &v["hits"][0];
    assert_eq!(h["path"], "src/cart.rs", "{v}");
    assert_eq!(h["tree"], json!(id));
    assert_eq!(h["edited_by"]["id"], json!(id));
    assert_eq!(v["agent"]["id"], json!(id));
    // The main checkout has no such line; its hit says who changed the file.
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=applyCouponCode"));
    assert_eq!(v["hits"], json!([]), "{v}");
    let (_, v) = s.get(&format!("/api/search/code?repo={repo}&q=cart_total"));
    let h = v["hits"].as_array().unwrap().iter().find(|h| h["path"] == "src/cart.rs").unwrap();
    assert_eq!(h["tree"], "main");
    assert_eq!(h["overlaid_by"][0]["id"], json!(id), "{h}");
    let (_, v) = s.get(&format!("/api/search/index?repo={repo}"));
    assert!(v["recent"].as_array().unwrap().iter().any(|x| x["why"] == "edit" && x["path"] == "src/cart.rs" && x["agent"]["id"] == json!(id)), "{v}");
    assert_eq!(v["index"]["overlays"][0]["agent"]["id"], json!(id), "{v}");
    assert_eq!(v["index"]["overlays"][0]["files"], json!(["src/cart.rs"]));
}

/// A doc site on loopback: a first page linking to pages in its folder,
/// one robots.txt disallows, one in another folder, and one elsewhere.
fn tiny_docs() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
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
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                seen.lock().unwrap().push(path.clone());
                let (status, ctype, body) = match path.as_str() {
                    "/robots.txt" => ("200 OK", "text/plain", "User-agent: *\nDisallow: /docs/private/\n".to_string()),
                    "/docs/" | "/docs/index.html" => ("200 OK", "text/html", "<html><head><title>Shop SDK</title></head><body><nav><a href=\"intro.html\">Intro</a> <a href=\"/docs/payments.html#top\">Payments</a> <a href=\"private/keys.html\">Keys</a> <a href=\"/blog/news.html\">News</a> <a href=\"https://example.com/x\">x</a> <a href=\"logo.png\">logo</a></nav><main><h1>Shop SDK</h1><p>Start here.</p></main></body></html>".to_string()),
                    "/docs/intro.html" => ("200 OK", "text/html; charset=utf-8", "<html><head><title>Intro &ndash; Shop SDK</title></head><body><main><h1>Intro</h1><p>Install the SDK and create a client.</p></main></body></html>".to_string()),
                    "/docs/payments.html" => ("200 OK", "text/html", "<html><head><title>PaymentIntent</title></head><body><main><h1>PaymentIntent</h1><p>Update a PaymentIntent's amount before it is confirmed. The amount is in the smallest currency unit, such as cents.</p></main></body></html>".to_string()),
                    _ => ("404 Not Found", "text/plain", "no".to_string()),
                };
                let _ = write!(c, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            });
        }
    });
    (port, seen)
}

fn have_curl() -> bool {
    Command::new("curl").arg("--version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn libraries_come_from_manifests_and_a_crawl_is_searched_and_cited() {
    let s = Server::start(&[]);
    let (_d, r) = shop_repo();
    let repo = enc(r.to_str().unwrap());
    let (st, v) = s.get(&format!("/api/library?repo={repo}"));
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["manifests"], json!(["Cargo.toml"]), "{v}");
    let serde = v["libraries"].as_array().unwrap().iter().find(|l| l["id"] == "crates-serde").unwrap_or_else(|| panic!("{v}"));
    assert_eq!(serde["version"], "1.0.214", "the lock's version: {serde}");
    assert_eq!(serde["url"], "https://docs.rs/serde/1.0.214/");
    assert_eq!(serde["state"], "none");
    assert_eq!(serde["manifest"], "Cargo.toml");
    let (_, v) = s.get(&format!("/api/search/docs?repo={repo}&q=amount"));
    assert_eq!(v["hits"], json!([]));
    assert!(v["missing"][0].as_str().unwrap().contains("No docs are crawled"), "{v}");
    if !have_curl() {
        assert_eq!(s.get(&format!("/api/library?repo={repo}")).1["crawler"]["available"], false);
        eprintln!("curl is not on PATH: the crawl is not tested");
        return;
    }
    assert_eq!(s.get(&format!("/api/library?repo={repo}")).1["crawler"]["available"], true);

    let (port, seen) = tiny_docs();
    let site = format!("http://127.0.0.1:{port}/docs/index.html");
    let (st, v) = s.post("/api/library/crawl", json!({ "repo": r, "url": site }));
    assert_eq!(st, 202, "{v}");
    let lib = v["library"]["id"].as_str().unwrap().to_string();
    assert!(lib.starts_with("site-127.0.0.1"), "{lib}");
    let v = s.get_until(&format!("/api/library?repo={repo}"), "the crawl is done", |v| v["libraries"].as_array().unwrap().iter().any(|l| l["id"] == json!(lib) && (l["state"] == "done" || l["state"] == "failed")));
    let l = v["libraries"].as_array().unwrap().iter().find(|l| l["id"] == json!(lib)).unwrap();
    assert_eq!(l["state"], "done", "{l}");
    assert_eq!(l["pages"], 3, "the first page, intro and payments: {l}");
    assert_eq!(l["ecosystem"], "site");
    let fetched = seen.lock().unwrap().clone();
    assert!(fetched.contains(&"/robots.txt".to_string()), "{fetched:?}");
    assert!(!fetched.iter().any(|p| p.starts_with("/docs/private") || p.starts_with("/blog") || p.ends_with(".png")), "robots.txt and the folder bound the crawl: {fetched:?}");
    let (_, pv) = s.get(&format!("/api/library/{lib}/pages?repo={repo}"));
    let mut urls: Vec<String> = pv["pages"].as_array().unwrap().iter().map(|p| p["url"].as_str().unwrap().to_string()).collect();
    urls.sort();
    let base = format!("http://127.0.0.1:{port}/docs/");
    assert_eq!(urls, [format!("{base}index.html"), format!("{base}intro.html"), format!("{base}payments.html")], "{pv}");
    assert!(pv["pages"].as_array().unwrap().iter().any(|p| p["title"] == "Intro – Shop SDK"), "{pv}");

    // An agent's search_docs answers URLs to cite, and the citation is kept.
    let lead = s.spawn(json!({ "repo": r, "name": "lead", "worktree": false }));
    let lid = lead["id"].as_str().unwrap().to_string();
    let mut m = Mcp::start(&s, &lid);
    let (text, err) = m.tool("search_docs", json!({ "query": "PaymentIntent amount" }));
    assert!(!err, "{text}");
    assert!(text.contains(&format!("<{base}payments.html>")) && text.contains("Cite the URLs"), "{text}");
    let (text, err) = m.tool("search_docs", json!({ "query": "amount currency", "tokens": 200 }));
    assert!(!err, "{text}");
    assert!(text.starts_with("PACK 200 TOKENS") && text.contains("smallest currency unit"), "{text}");
    let (text, err) = m.tool("search_code", json!({ "query": "cartTotal" }));
    assert!(!err, "{text}");
    assert!(text.contains("src/cart.rs:1-6 fn cart_total") && text.contains("web/useCart.ts:"), "{text}");
    let (text, _) = m.tool("search_code", json!({ "query": "round discount", "tokens": 150 }));
    assert!(text.starts_with("PACK 150 TOKENS") && text.contains("// src/cart.rs:"), "{text}");

    let (_, v) = s.get(&format!("/api/library?repo={repo}"));
    let c = &v["citations"];
    assert!(c.as_array().unwrap().iter().any(|c| c["agent"]["id"] == json!(lid) && c["url"] == json!(format!("{base}payments.html")) && c["q"] == "PaymentIntent amount"), "{v}");
    let l = v["libraries"].as_array().unwrap().iter().find(|l| l["id"] == json!(lib)).unwrap();
    assert!(l["cited"].as_i64().unwrap() >= 1 && l["cited_by"][0]["id"] == json!(lid), "{l}");
    let (_, pv) = s.get(&format!("/api/library/{lib}/pages?repo={repo}"));
    assert_eq!(pv["pages"][0]["url"], json!(format!("{base}payments.html")), "the cited page first: {pv}");
    assert_eq!(pv["pages"][0]["cited_by"][0]["agent"]["id"], json!(lid));
    let (_, v) = s.get(&format!("/api/search/index?repo={repo}"));
    let kinds: Vec<&str> = v["searches"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"code") && kinds.contains(&"docs"), "{v}");
    assert!(v["searches"].as_array().unwrap().iter().all(|x| x["agent"]["id"] == json!(lid)));

    // The quick search: code, docs and (without ChloeCtx) no decisions.
    let (st, v) = s.get(&format!("/api/search/quick?repo={repo}&q=PaymentIntent%20amount"));
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["docs"][0]["url"], json!(format!("{base}payments.html")), "{v}");
    assert_eq!(v["decisions"], json!([]));
    let (_, v) = s.get(&format!("/api/search/quick?repo={repo}&q=cartTotal"));
    assert!(!v["code"].as_array().unwrap().is_empty(), "{v}");
    let (st, _) = s.post("/api/library/crawl", json!({ "repo": r }));
    assert_eq!(st, 400);
    let (st, _) = s.post("/api/library/crawl", json!({ "repo": r, "library": "crates-nope" }));
    assert_eq!(st, 404);
}

// Fourth build: Preview lab (use cases 2 and 3): persona tests through the
// preview, and runtime errors joined to the edit that probably caused them.

/// A server with extra environment (the persona runner's, here).
fn start_lab(envs: &[(&str, &str)]) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let fake = write_fake(dir.path());
    let mut child = Command::new(BIN)
        .args(["serve", "--addr", "127.0.0.1:0", "--token", TOKEN])
        .arg("--data-dir")
        .arg(&data)
        .arg("--claude-bin")
        .arg(&fake)
        .args(["--chloectx-bin", "/nonexistent/chloectx", "--no-chloectx"])
        .envs(envs.iter().copied())
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

/// An app that answers every request with `status` and `body`.
fn failing_app(status: u16, body: String) -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut c) = conn else { continue };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(c.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let _ = write!(c, "HTTP/1.1 {status} Oops\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            });
        }
    });
    port
}

fn error_of<'a>(list: &'a Value, source: &str) -> &'a Value {
    list["errors"].as_array().unwrap().iter().find(|e| e["source"] == source).unwrap_or_else(|| panic!("no {source} error in {list}"))
}

#[test]
fn runtime_errors_are_recorded_and_put_down_to_an_agents_edit() {
    let s = start_lab(&[]);
    let (_d, r) = repo();
    let web = s.spawn(json!({ "repo": r, "name": "web", "task": "WRITE money.js function total(cart) { return cart.items.length }" }));
    let api = s.spawn(json!({ "repo": r, "name": "api", "task": "WRITE server/orders.js function placeOrder() { throw new Error(boom) }" }));
    let (web, api) = (web["id"].as_str().unwrap().to_string(), api["id"].as_str().unwrap().to_string());
    s.wait(&web, "idle");
    s.wait(&api, "idle");
    let api_cwd = s.agent(&api)["cwd"].as_str().unwrap().to_string();
    assert_ne!(api_cwd, s.agent(&web)["cwd"].as_str().unwrap(), "each in its own worktree");
    let (_, v) = s.get(&format!("/api/runtime/errors?agent={web}"));
    assert_eq!(v["errors"], json!([]), "{v}");
    assert!(v["otlp"]["endpoint"].as_str().unwrap().ends_with("/v1/logs"), "{v}");

    // A 5xx through the preview, whose stack names api's file in its worktree.
    let port = failing_app(500, format!("Error: order failed\n    at placeOrder ({api_cwd}/server/orders.js:1:30)\n    at node:internal/process:1:1\n"));
    assert_eq!(s.put(&format!("/api/agents/{web}/preview"), json!({ "url": format!("http://127.0.0.1:{port}") })).0, 200);
    let jar = format!("wb_preview={TOKEN}");
    let mut res = http_raw().get(format!("{}/preview/{web}/api/orders", s.url)).header("cookie", &jar).call().unwrap();
    assert_eq!(res.status().as_u16(), 500);
    assert!(res.body_mut().read_to_string().unwrap().starts_with("Error: order failed"), "the answer goes on unchanged");
    let (_, v) = s.get(&format!("/api/runtime/errors?agent={web}"));
    assert_eq!(v["missing"], json!([]), "the ASOF JOIN ran: {v}");
    let e = error_of(&v, "proxy");
    assert_eq!((e["status"].as_i64(), e["message"].as_str(), e["file"].as_str(), e["line"].as_i64()), (Some(500), Some("500 GET /api/orders: Error: order failed"), Some("server/orders.js"), Some(1)), "{e}");
    assert_eq!(e["cwd"], json!(api_cwd));
    let c = &e["culprit"];
    assert_eq!((c["agent_id"].as_str(), c["how"].as_str(), c["file"].as_str(), c["line"].as_i64()), (Some(api.as_str()), Some("stack"), Some("server/orders.js"), Some(1)), "{c}");
    assert!(c["edit_ms"].as_i64().unwrap() <= e["first_ms"].as_i64().unwrap());
    let frames = e["frames"].as_array().unwrap();
    assert_eq!((frames[0]["in_repo"].as_bool(), frames[1]["in_repo"].as_bool()), (Some(true), Some(false)), "{frames:?}");

    // What the page threw, from the overlay, on the preview's cookie; a
    // repeat is counted, not added.
    let report = json!({ "kind": "error", "message": "TypeError: cart.items is undefined", "stack": format!("TypeError: cart.items is undefined\n    at total (http://127.0.0.1:{port}/preview/{web}/money.js:1:40)\n    at http://127.0.0.1:{port}/preview/{web}/node_modules/react/index.js:9:1"), "filename": format!("http://127.0.0.1:{port}/preview/{web}/money.js"), "lineno": 1, "colno": 40, "url": "/cart" });
    let overlay = format!("{}/preview/{web}/__wb/runtime", s.url);
    assert_eq!(http_raw().post(&overlay).header("origin", &s.url).send_json(&report).unwrap().status().as_u16(), 401);
    for _ in 0..2 {
        let mut res = http_raw().post(&overlay).header("cookie", &jar).header("origin", &s.url).send_json(&report).unwrap();
        assert_eq!(res.status().as_u16(), 200);
        assert_eq!(res.body_mut().read_json::<Value>().unwrap()["ok"], true);
    }
    http_raw().post(&overlay).header("cookie", &jar).header("origin", &s.url).send_json(json!({ "kind": "console", "message": "Warning: Each child in a list should have a unique key prop." })).unwrap();
    // One with no stack: the last edit in its worktree, by time.
    let (status, v) = s.post("/api/runtime/errors", json!({ "agent": web, "message": "Checkout hangs" }));
    assert_eq!(status, 201, "{v}");
    assert_eq!(v["error"]["source"], "api");
    let (_, v) = s.get(&format!("/api/runtime/errors?agent={web}"));
    assert_eq!(v["total"], 4, "{v}");
    let e = error_of(&v, "overlay");
    assert_eq!((e["count"].as_i64(), e["file"].as_str(), e["line"].as_i64(), e["col"].as_i64(), e["url"].as_str()), (Some(2), Some("money.js"), Some(1), Some(40), Some("/cart")), "{e}");
    assert_eq!((e["culprit"]["agent_id"].as_str(), e["culprit"]["how"].as_str()), (Some(web.as_str()), Some("stack")), "{e}");
    assert!(e["frames"].as_array().unwrap().iter().any(|f| f["file"].as_str().unwrap().contains("node_modules") && f["in_repo"] == false), "{e}");
    assert_eq!(error_of(&v, "console")["level"], "warn");
    let e = error_of(&v, "api");
    assert_eq!((e["culprit"]["agent_id"].as_str(), e["culprit"]["how"].as_str(), e["culprit"]["file"].as_str()), (Some(web.as_str()), Some("time"), Some("money.js")), "{e}");

    // OTLP/HTTP JSON logs: the token, JSON only, errors only, and the agent
    // from the service's name.
    let logs = format!("{}/v1/logs", s.url);
    let otlp = json!({ "resourceLogs": [
        { "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": "api" } }] },
          "scopeLogs": [{ "logRecords": [
            { "severityNumber": 9, "severityText": "INFO", "body": { "stringValue": "listening on 8080" } },
            { "severityNumber": 17, "severityText": "ERROR", "body": { "stringValue": "insert failed" }, "attributes": [
                { "key": "exception.type", "value": { "stringValue": "DbError" } },
                { "key": "exception.message", "value": { "stringValue": "connection refused" } },
                { "key": "exception.stacktrace", "value": { "stringValue": format!("DbError: connection refused\n    at placeOrder ({api_cwd}/server/orders.js:1:12)") } },
                { "key": "url.path", "value": { "stringValue": "/api/orders" } } ] } ] }] },
        { "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": "ghost" } }] },
          "scopeLogs": [{ "logRecords": [{ "severityNumber": 18, "body": { "stringValue": "who am I" } }] }] }
    ]});
    assert_eq!(http_raw().post(&logs).send_json(&otlp).unwrap().status().as_u16(), 401);
    assert_eq!(http_raw().post(&logs).header("authorization", format!("Bearer {TOKEN}")).header("content-type", "application/x-protobuf").send("x").unwrap().status().as_u16(), 415);
    let mut res = http_raw().post(&logs).header("authorization", format!("Bearer {TOKEN}")).send_json(&otlp).unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let v: Value = res.body_mut().read_json().unwrap();
    assert_eq!(v["partialSuccess"]["rejectedLogRecords"], 1, "{v}");
    let (_, v) = s.get(&format!("/api/runtime/errors?agent={api}"));
    let e = error_of(&v, "otlp");
    assert_eq!((e["message"].as_str(), e["service"].as_str(), e["url"].as_str(), e["file"].as_str()), (Some("DbError: connection refused"), Some("api"), Some("/api/orders"), Some("server/orders.js")), "{e}");
    assert_eq!(e["culprit"]["agent_id"], json!(api));
    let (_, v) = s.get(&format!("/api/runtime/errors?repo={}", r.display()));
    assert_eq!(v["total"], 5, "{v}");

    // An agent reads them with runtime_errors.
    let mut m = Mcp::start(&s, &web);
    let (text, err) = m.tool("runtime_errors", json!({}));
    assert!(!err, "{text}");
    assert!(text.contains("500 GET /api/orders") && text.contains("probably api's edit to server/orders.js") && !text.contains("connection refused"), "{text}");
    let (text, _) = m.tool("runtime_errors", json!({ "all": true }));
    assert!(text.contains("DbError: connection refused"), "{text}");

    // Resolved, it leaves the open list.
    let (_, v) = s.get(&format!("/api/runtime/errors?agent={web}"));
    let id = error_of(&v, "proxy")["id"].as_str().unwrap().to_string();
    let (status, v) = s.post(&format!("/api/runtime/errors/{id}/resolve"), json!({}));
    assert_eq!((status, v["error"]["state"].as_str()), (200, Some("resolved")), "{v}");
    assert_eq!(s.get(&format!("/api/runtime/errors?agent={web}")).1["total"], 3);
    assert_eq!(s.get(&format!("/api/runtime/errors?agent={web}&state=resolved")).1["total"], 1);
    // Bad reports.
    assert_eq!(s.post("/api/runtime/errors", json!({ "message": "x" })).0, 400);
    assert_eq!(s.post("/api/runtime/errors", json!({ "agent": "a00000000", "message": "x" })).0, 404);
    assert_eq!(s.post("/api/runtime/errors", json!({ "agent": web, "message": " " })).0, 400);
    assert_eq!(s.post("/api/runtime/errors/e00000000/resolve", json!({})).0, 404);
}

/// The persona runner, faked: it answers `--check`, opens the preview with
/// the token it was given, and reports steps, findings and the end. Its
/// task picks what it does: SLOW plays until stopped, CRASH fails.
const FAKE_RUNNER: &str = r#"#!/usr/bin/env python3
import json, os, sys, urllib.request, http.cookiejar
if "--check" in sys.argv:
    print(json.dumps({"ok": True, "why": None, "adapter": "fake"}))
    sys.exit(0)
job = json.loads(sys.stdin.read())
def out(o):
    print(json.dumps(o), flush=True)
out({"type": "start", "adapter": "fake"})
if "CRASH" in job["task"]:
    sys.stderr.write("boom: no browser here\n")
    sys.exit(1)
op = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
page = op.open(job["url"] + "/?token=" + job["token"]).read().decode()
clean = "CHLOEDB_WORKBENCH_TOKEN" not in os.environ and "CHLOEDB_WORKBENCH_AGENT" not in os.environ
out({"type": "step", "step": 1, "action": "click", "input": {"ref": "e1", "name": "Buy"}, "thought": "Buying it" + (" (clean)" if clean else ""), "ok": True, "url": job["url"] + "/", "events": [], "ms": 5})
if "SLOW" in job["task"]:
    os.execvp("sleep", ["sleep", "60"])
out({"type": "finding", "kind": "bug", "severity": 3, "title": "Pay does nothing", "detail": "The page said " + ("Hello preview" if "Hello preview" in page else "nothing"), "step": 1, "url": job["url"] + "/"})
out({"type": "finding", "kind": "slow", "severity": 2, "title": "Checkout is slow", "detail": "It took a while", "step": 1})
out({"type": "finding", "kind": "copy", "severity": 1, "title": "The labels are small", "detail": "Hard to read"})
out({"type": "done", "outcome": "done", "answer": "I bought it as " + job["persona"]["name"] + " with " + job["model"], "cost_usd": 0.01})
"#;

fn run_until(s: &Server, id: &str, f: impl Fn(&Value) -> bool) -> Value {
    s.get_until(&format!("/api/swarm/runs/{id}"), "the run's state", f)
}

fn finding<'a>(run: &'a Value, title: &str) -> &'a Value {
    run["findings"].as_array().unwrap().iter().find(|f| f["title"] == title).unwrap_or_else(|| panic!("no finding {title} in {run}"))
}

#[test]
fn a_persona_tests_the_preview_and_its_findings_go_to_the_agent_or_to_decide() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let runner = bin.path().join("fake-runner");
    std::fs::write(&runner, FAKE_RUNNER).unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
    let s = start_lab(&[("CHLOEDB_WORKBENCH_SWARM_CMD", runner.to_str().unwrap())]);
    let (_d, r) = repo();
    let (port, _seen) = tiny_app();
    let shop = s.spawn(json!({ "repo": r, "name": "shop", "task": "WRITE page.html <h1>Hello preview</h1>", "worktree": false }));
    let other = s.spawn(json!({ "repo": r, "name": "other", "task": "WRITE other.txt x", "worktree": false }));
    let (id, other) = (shop["id"].as_str().unwrap().to_string(), other["id"].as_str().unwrap().to_string());
    s.wait(&id, "idle");
    s.wait(&other, "idle");

    let (_, v) = s.get("/api/swarm/personas");
    assert_eq!((v["available"].as_bool(), v["adapter"].as_str()), (Some(true), Some("fake")), "{v}");
    let ids: Vec<&str> = v["personas"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["first-timer", "keyboard", "careful", "hurried", "edge-cases"]);
    // Only an agent with a preview, a task and a persona.
    assert_eq!(s.post("/api/swarm/runs", json!({ "agent": other, "persona": "careful", "task": "buy" })).0, 409);
    assert_eq!(s.post("/api/swarm/runs", json!({ "agent": "a00000000", "persona": "careful", "task": "buy" })).0, 404);
    assert_eq!(s.put(&format!("/api/agents/{id}/preview"), json!({ "url": format!("http://127.0.0.1:{port}") })).0, 200);
    assert_eq!(s.post("/api/swarm/runs", json!({ "agent": id, "persona": "careful" })).0, 400);
    assert_eq!(s.post("/api/swarm/runs", json!({ "agent": id, "persona": "nobody", "task": "buy" })).0, 400);
    assert_eq!(s.post("/api/swarm/runs", json!({ "agent": id, "task": "buy" })).0, 400);

    // A run: its steps, findings and end, from the runner's lines.
    let (status, v) = s.post("/api/swarm/runs", json!({ "agent": id, "persona": "careful", "task": "Buy a mug", "model": "sonnet" }));
    assert_eq!(status, 201, "{v}");
    assert_eq!((v["run"]["state"].as_str(), v["run"]["by"].as_str()), (Some("starting"), Some("you")), "{v}");
    let rid = v["run"]["id"].as_str().unwrap().to_string();
    let v = run_until(&s, &rid, |v| v["run"]["state"] == "done");
    let run = &v["run"];
    assert_eq!((run["outcome"].as_str(), run["answer"].as_str(), run["adapter"].as_str()), (Some("done"), Some("I bought it as Ruth Alvarez with sonnet"), Some("fake")), "{v}");
    assert_eq!(run["persona"]["name"], "Ruth Alvarez");
    assert_eq!(run["step_count"], 1);
    assert_eq!(run["cost_usd"], 0.01);
    let step = &run["steps"][0];
    assert_eq!((step["action"].as_str(), step["input"]["name"].as_str(), step["thought"].as_str()), (Some("click"), Some("Buy"), Some("Buying it (clean)")), "the runner gets no token in its environment: {step}");
    assert_eq!(v["findings"].as_array().unwrap().len(), 3, "{v}");
    let pay = finding(&v, "Pay does nothing");
    assert_eq!((pay["severity"].as_i64(), pay["state"].as_str(), pay["detail"].as_str()), (Some(3), Some("open"), Some("The page said Hello preview")), "it opened the preview: {pay}");
    let (_, list) = s.get(&format!("/api/swarm/runs?agent={id}"));
    assert_eq!(list["runs"][0]["id"], json!(rid), "{list}");
    assert_eq!(list["runs"][0]["findings_count"], 3, "{list}");

    // Send: a steer to the preview's agent.
    let fid = pay["id"].as_str().unwrap().to_string();
    let (status, a) = s.post(&format!("/api/swarm/findings/{fid}/send"), json!({ "text": "the button is the green one" }));
    assert_eq!(status, 200, "{a}");
    assert_eq!(a["finding"]["state"], "sent");
    assert!(a["finding"]["steer"].is_string(), "{a}");
    let steered = |want: &str| s.events(&id).iter().any(|e| e["kind"] == "steer" && e.to_string().contains(want));
    assert!(steered("Pay does nothing") && steered("the button is the green one"));
    // Ask: a question in Decide; its answer sends the finding.
    let slow = finding(&v, "Checkout is slow")["id"].as_str().unwrap().to_string();
    let (status, a) = s.post(&format!("/api/swarm/findings/{slow}/ask"), json!({}));
    assert_eq!(status, 200, "{a}");
    assert_eq!(a["finding"]["state"], "asked");
    assert_eq!(s.post(&format!("/api/swarm/findings/{slow}/ask"), json!({})).0, 409);
    let qs = s.get_until(&format!("/api/questions?state=open&agent={id}"), "the finding's question", |v| v["questions"].as_array().is_some_and(|q| q.len() == 1));
    let q = &qs["questions"][0];
    assert_eq!((q["kind"].as_str(), q["recommended"].as_i64()), (Some("ask"), Some(0)), "{q}");
    assert!(q["text"].as_str().unwrap().contains("Checkout is slow"), "{q}");
    assert_eq!(q["options"][0]["label"], "Send it to shop");
    let (status, _) = s.post(&format!("/api/questions/{}/answer", q["id"].as_str().unwrap()), json!({ "option": 0, "text": "make it fast" }));
    assert_eq!(status, 200);
    run_until(&s, &rid, |v| finding(v, "Checkout is slow")["state"] == "sent");
    assert!(steered("Checkout is slow") && steered("make it fast"));
    // Dismiss.
    let small = finding(&v, "The labels are small")["id"].as_str().unwrap().to_string();
    let (_, a) = s.post(&format!("/api/swarm/findings/{small}/dismiss"), json!({}));
    assert_eq!(a["finding"]["state"], "dismissed");
    assert_eq!(s.post(&format!("/api/swarm/findings/{small}/burn"), json!({})).0, 400);
    assert_eq!(s.post("/api/swarm/findings/f00000000/send", json!({})).0, 404);

    // A person in words; one run at a time per preview; Stop.
    let (status, v) = s.post("/api/swarm/runs", json!({ "agent": id, "persona_text": "a night-shift nurse on her phone", "task": "SLOW browse" }));
    assert_eq!(status, 201, "{v}");
    assert_eq!((v["run"]["persona"]["role"].as_str(), v["run"]["persona"]["device"].as_str()), (Some("a night-shift nurse on her phone"), Some("phone")));
    let slow_run = v["run"]["id"].as_str().unwrap().to_string();
    run_until(&s, &slow_run, |v| v["run"]["step_count"] == 1);
    assert_eq!(s.post("/api/swarm/runs", json!({ "agent": id, "persona": "hurried", "task": "again" })).0, 409);
    let (status, v) = s.post(&format!("/api/swarm/runs/{slow_run}/stop"), json!({}));
    assert_eq!(status, 200, "{v}");
    let v = run_until(&s, &slow_run, |v| v["run"]["ended_ms"].is_i64());
    assert_eq!((v["run"]["state"].as_str(), v["run"]["outcome"].as_str()), (Some("stopped"), Some("stopped")), "{v}");
    // A runner that fails: the run says why.
    let (_, v) = s.post("/api/swarm/runs", json!({ "agent": id, "persona": "hurried", "task": "CRASH it" }));
    let crash = v["run"]["id"].as_str().unwrap().to_string();
    let v = run_until(&s, &crash, |v| v["run"]["state"] == "failed");
    assert!(v["run"]["error"].as_str().unwrap().contains("boom: no browser here"), "{v}");

    // An agent tests its own preview with test_with_persona, and waits.
    let mut m = Mcp::start(&s, &id);
    let (text, err) = m.tool("test_with_persona", json!({ "task": "Buy a mug", "persona": "keyboard" }));
    assert!(!err, "{text}");
    assert!(text.contains("Ken Okafor") && text.contains("done after 1 steps") && text.contains("[blocker] Pay does nothing"), "{text}");
    let (_, list) = s.get(&format!("/api/swarm/runs?agent={id}"));
    assert_eq!(list["runs"][0]["by"], json!(id), "{list}");
    let (text, err) = m.tool("test_with_persona", json!({}));
    assert!(err && text.contains("task is required"), "{text}");
}

#[test]
fn persona_tests_say_why_they_cannot_run() {
    let s = start_lab(&[("CHLOEDB_WORKBENCH_SWARM_CMD", "/nonexistent/swarm-runner")]);
    let (_d, r) = repo();
    let (port, _seen) = tiny_app();
    let a = s.spawn(json!({ "repo": r, "name": "shop", "worktree": false }));
    let id = a["id"].as_str().unwrap().to_string();
    let (_, v) = s.get("/api/swarm/personas");
    assert_eq!(v["available"], false, "{v}");
    assert!(v["why"].as_str().unwrap().contains("cannot run /nonexistent/swarm-runner"), "{v}");
    assert_eq!(v["personas"].as_array().unwrap().len(), 5);
    assert_eq!(s.put(&format!("/api/agents/{id}/preview"), json!({ "url": format!("http://127.0.0.1:{port}") })).0, 200);
    let (status, v) = s.post("/api/swarm/runs", json!({ "agent": id, "persona": "careful", "task": "buy" }));
    assert_eq!((status, v["available"].as_bool(), v["run"].is_null()), (200, Some(false), true), "{v}");
    assert_eq!(s.get(&format!("/api/swarm/runs?agent={id}")).1["runs"], json!([]));
    let mut m = Mcp::start(&s, &id);
    let (text, err) = m.tool("test_with_persona", json!({ "task": "buy", "persona": "a man in a hurry" }));
    assert!(err && text.contains("not available"), "{text}");
}

// Fourth build: Trace (a question in a sentence, answered with where it
// happened: commits, edits, decisions, answers and rules, the code now).

fn item<'a>(t: &'a Value, f: impl Fn(&Value) -> bool) -> Option<&'a Value> {
    t["timeline"].as_array().unwrap().iter().find(|i| f(i))
}

fn enc_q(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[test]
fn trace_finds_where_x_became_y_who_made_it_and_why() {
    let ctx = tempfile::tempdir().unwrap();
    let bin = fake_chloectx(ctx.path());
    let s = Server::start(&["--chloectx-bin", bin.to_str().unwrap()]);
    let (_d, r) = repo();
    std::fs::write(r.join("calc.py"), "MODE = floor\n").unwrap();
    git(&r, &["add", "calc.py"]);
    git(&r, &["commit", "-q", "-m", "calc: rounding mode floor"]);

    // cart writes the new mode with its Edit tool, then commits on its branch.
    let cart = s.spawn(json!({ "repo": r, "name": "cart", "task": "WRITE calc.py MODE = half_up" }));
    let cart_id = cart["id"].as_str().unwrap().to_string();
    let cart = s.wait(&cart_id, "idle");
    let cwd = PathBuf::from(cart["cwd"].as_str().unwrap());
    std::fs::write(cwd.join("calc.py"), "MODE = half_up\n").unwrap();
    git(&cwd, &["commit", "-q", "-am", "calc: round half up"]);
    let sha = git(&cwd, &["rev-parse", "HEAD"]);
    // web writes it too, and does not commit.
    let web = s.spawn(json!({ "repo": r, "name": "web", "task": "WRITE web.js const MODE = half_up" }));
    let web_id = web["id"].as_str().unwrap().to_string();
    s.wait(&web_id, "idle");
    // cart asked, and you answered and made a rule.
    let (status, q) = s.post("/api/questions", json!({ "agent": cart_id, "question": "Keep floor or round half_up?", "options": ["half_up", "floor"] }));
    assert_eq!(status, 201, "{q}");
    let qid = q["question"]["id"].as_str().unwrap().to_string();
    let (status, v) = s.post(&format!("/api/questions/{qid}/answer"), json!({ "option": 0, "rule": "Totals round half_up, never floor" }));
    assert_eq!(status, 200, "{v}");

    // ChloeCtx: what ask found, the records sql answers (one about it from
    // cart's session, one about something else).
    let sid = cart["session_id"].as_str().unwrap().to_string();
    let sid8: String = sid.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    let now = now_s();
    std::fs::write(ctx.path().join("ask.json"), json!({ "text": "- [decision] Round half_up", "ids": [format!("{sid8}:9")], "ms": 2 }).to_string()).unwrap();
    std::fs::write(
        ctx.path().join("sql.json"),
        json!({ "text": "", "count": 2, "columns": ["id", "kind", "tag", "ts", "session", "text", "files", "links"], "rows": [
            [format!("{sid8}:9"), "decision", "", now, sid, "Round MODE half_up: floor lost a cent on 3 carts", "calc.py", null],
            ["other000:4", "decision", "", now, "other-session", "Use tabs in the Makefile", "Makefile", null]
        ]})
        .to_string(),
    )
    .unwrap();

    let question = "when and why did we change MODE from floor to half_up, and how does it work now?";
    let (status, t) = s.get(&format!("/api/trace?repo={}&q={}", enc_q(&r.to_string_lossy()), enc_q(question)));
    assert_eq!(status, 200, "{t}");
    // The reading.
    let rd = &t["reading"];
    assert_eq!((rd["x"].as_str(), rd["y"].as_str(), rd["topic"].as_str()), (Some("floor"), Some("half_up"), Some("MODE")), "{rd}");
    assert_eq!(rd["kinds"], json!(["when", "why", "how"]));
    assert!(rd["chips"].as_array().unwrap().iter().any(|c| c["key"] == "x" && c["text"] == "floor"), "{rd}");
    // When: cart's commit, floor out and half_up in, written by its edit.
    let c = item(&t, |i| i["kind"] == "commit" && i["commit"]["sha"] == sha.as_str()).unwrap_or_else(|| panic!("no commit {sha}: {t}"));
    assert_eq!(c["change"], "changed", "{c}");
    assert_eq!(c["agent"]["name"], "cart", "{c}");
    assert!(c["commit"]["branches"].as_array().unwrap().contains(&json!("wb/cart")), "{c}");
    assert_eq!(c["written"]["tool"], "Edit", "{c}");
    assert_eq!(c["written"]["file"], "calc.py");
    let signs: Vec<(&str, &str)> = c["lines"].as_array().unwrap().iter().map(|l| (l["sign"].as_str().unwrap(), l["text"].as_str().unwrap())).collect();
    assert_eq!(signs, [("-", "MODE = floor"), ("+", "MODE = half_up")]);
    assert_eq!(c["links"]["why"], json!({ "agent": cart_id, "path": "calc.py", "line": 1 }));
    assert_eq!(c["links"]["story"]["agent"], json!(cart_id));
    assert!(c["links"]["rewind"]["from_ms"].is_i64(), "{c}");
    assert_eq!(c["key"], true);
    // Where floor came in counts; the README's commit does not.
    let first = item(&t, |i| i["kind"] == "commit" && i["title"] == "calc: rounding mode floor").unwrap_or_else(|| panic!("{t}"));
    assert_eq!((first["change"].as_str(), first["agent"].is_null()), (Some("added"), true), "{first}");
    assert!(item(&t, |i| i["title"] == "first").is_none());
    // cart's edit is folded into its commit; web's is not committed.
    let e = item(&t, |i| i["kind"] == "edit").unwrap_or_else(|| panic!("{t}"));
    assert_eq!((e["agent"]["name"].as_str(), e["committed"].as_bool(), e["change"].as_str()), (Some("web"), Some(false), Some("added")), "{e}");
    assert_eq!(e["links"]["why"]["agent"], json!(web_id));
    assert_eq!(t["timeline"].as_array().unwrap().iter().filter(|i| i["kind"] == "edit").count(), 1, "{t}");
    // Why: cart's decision (not the other), your answer, the rule.
    let d = item(&t, |i| i["kind"] == "decision").unwrap_or_else(|| panic!("{t}"));
    assert_eq!((d["ctx"]["id"].as_str(), d["agent"]["name"].as_str()), (Some(format!("{sid8}:9").as_str()), Some("cart")), "{d}");
    assert!(item(&t, |i| i["ctx"]["id"] == "other000:4").is_none(), "{t}");
    let qi = item(&t, |i| i["kind"] == "question").unwrap_or_else(|| panic!("{t}"));
    assert_eq!((qi["question"]["answer"].as_str(), qi["by"].as_str()), (Some("half_up"), Some("you")), "{qi}");
    assert!(item(&t, |i| i["kind"] == "rule" && i["title"] == "Totals round half_up, never floor").is_some(), "{t}");
    // Tags in time order.
    let tags: Vec<&str> = t["timeline"].as_array().unwrap().iter().map(|i| i["tag"].as_str().unwrap()).collect();
    assert_eq!(tags[0], "E1");
    let ts: Vec<i64> = t["timeline"].as_array().unwrap().iter().map(|i| i["ts_ms"].as_i64().unwrap()).collect();
    assert!(ts.windows(2).all(|w| w[0] <= w[1]), "{ts:?}");
    // The answer.
    let a = t["answer"]["text"].as_str().unwrap();
    assert!(a.starts_with("The MODE went from floor to half_up {when} in cart's edit to calc.py (committed as "), "{a}");
    assert_eq!(t["answer"]["key"], c["id"]);
    assert!(t["answer"]["plain"].as_str().unwrap().contains(" UTC "), "{}", t["answer"]);
    // How it works now: half_up is only in cart's worktree (and web's).
    let code = &t["now"]["code"][0];
    assert_eq!((code["path"].as_str(), code["tree"].as_str(), t["now"]["source"].as_str()), (Some("calc.py"), Some("cart"), Some("git grep")), "{}", t["now"]);
    assert_eq!(code["hits"], json!([1]));
    // ChloeCtx was asked the question, and for the records naming it.
    assert_eq!(args_of(ctx.path(), "ask")[1..], ["ask", "--json", "--", question]);
    let sql = args_of(ctx.path(), "sql");
    assert!(sql[3].contains("MATCH(body, '") && sql[3].contains("half") && sql[3].contains(&format!("'{sid8}:9'")), "{sql:?}");

    // An edited reading: y alone, in another place: nothing there.
    let (_, t2) = s.get(&format!("/api/trace?repo={}&q=why&x=&y=half_up&in=docs/", enc_q(&r.to_string_lossy())));
    assert_eq!((t2["reading"]["x"].is_null(), t2["reading"]["paths"].clone()), (true, json!(["docs/"])), "{}", t2["reading"]);
    assert_eq!(t2["timeline"].as_array().unwrap().iter().filter(|i| i["kind"] == "commit").count(), 0, "{t2}");
    // A commit's diff.
    let (status, d) = s.get(&format!("/api/trace/commit?repo={}&sha={sha}", enc_q(&r.to_string_lossy())));
    assert_eq!(status, 200, "{d}");
    assert_eq!((d["subject"].as_str(), d["files"][0]["path"].as_str()), (Some("calc: round half up"), Some("calc.py")));
    assert_eq!(s.get(&format!("/api/trace/commit?repo={}&sha=nothex", enc_q(&r.to_string_lossy()))).0, 400);
    assert_eq!(s.get(&format!("/api/trace?repo={}", enc_q(&r.to_string_lossy()))).0, 400);

    // Explain asks ChloeCtx with a model, with the evidence to cite.
    let (status, x) = s.post("/api/trace/explain", json!({ "q": question, "repo": r }));
    assert_eq!(status, 200, "{x}");
    assert_eq!(x["available"], true);
    let ask = args_of(ctx.path(), "ask");
    assert_eq!(ask[1..4], ["ask", "--deep", "--json"]);
    assert!(ask.join("\n").contains("[E1]") && ask.join("\n").contains("calc: round half up"), "{ask:?}");

    // Agents ask it too, as text with citations.
    let mut m = Mcp::start(&s, &cart_id);
    let (text, err) = m.tool("trace", json!({ "question": question }));
    assert!(!err, "{text}");
    assert!(text.starts_with("Answer: The MODE went from floor to half_up on "), "{text}");
    assert!(text.contains("[E1]") && text.contains("calc: round half up") && text.contains("How it works now"), "{text}");
}

#[test]
fn trace_without_chloectx_answers_from_git() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    std::fs::create_dir(r.join("api")).unwrap();
    std::fs::write(r.join("api/client.rs"), "/// How often an upload is tried.\npub fn retries() -> u32 {\n    let retries = 3;\n    retries\n}\n").unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "client: retry uploads"]);
    std::fs::write(r.join("api/client.rs"), "/// How often an upload is tried.\npub fn retries() -> u32 {\n    let retries = 5;\n    retries\n}\n").unwrap();
    git(&r, &["commit", "-q", "-am", "client: retry 5 times, uploads are flaky"]);

    let (status, t) = s.get(&format!("/api/trace?repo={}&q={}", enc_q(&r.to_string_lossy()), enc_q("when did we change `retries` from 3 to 5 in api/?")));
    assert_eq!(status, 200, "{t}");
    assert_eq!((t["reading"]["x"].as_str(), t["reading"]["y"].as_str()), (Some("3"), Some("5")), "{}", t["reading"]);
    assert_eq!(t["reading"]["paths"], json!(["api/"]));
    assert_eq!(t["reading"]["needles"], json!(["retries … 3", "retries … 5", "retries"]));
    let c = item(&t, |i| i["title"] == "client: retry 5 times, uploads are flaky").unwrap_or_else(|| panic!("{t}"));
    assert_eq!((c["change"].as_str(), c["by"].as_str(), c["agent"].is_null()), (Some("changed"), Some("Test"), true), "{c}");
    assert_eq!(c["links"]["why"], json!({ "repo": r, "path": "api/client.rs", "line": 3 }));
    assert!(t["answer"]["text"].as_str().unwrap().starts_with("The retries went from 3 to 5 {when} in Test's commit "), "{}", t["answer"]);
    // No ChloeCtx: no decisions, and it says so.
    assert!(item(&t, |i| i["kind"] == "decision").is_none());
    assert!(t["missing"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().contains("ChloeCtx is not installed")), "{}", t["missing"]);
    // The code now: the function around the line, from git grep.
    let code = &t["now"]["code"][0];
    assert_eq!((code["path"].as_str(), code["symbol"].as_str(), code["start_line"].as_i64(), code["end_line"].as_i64()), (Some("api/client.rs"), Some("retries"), Some(1), Some(5)), "{code}");
    assert_eq!(code["hits"], json!([3]));
    // Explain says what is missing instead of failing.
    let (status, x) = s.post("/api/trace/explain", json!({ "q": "why is `retries` 5?", "repo": r }));
    assert_eq!((status, x["available"].as_bool()), (200, Some(false)), "{x}");
}

// Fifth build: Hotspots (the files the agents keep correcting, redoing or
// breaking, scored from Workbench's rows, and told to the next agent once).

/// An edit as the PostToolUse hook of Claude Code's Edit tool sends it.
fn edit_as(s: &Server, id: &str, file: &Path, old: &str, new: &str) {
    hook_as(s, id, json!({ "hook_event_name": "PostToolUse", "tool_name": "Edit", "tool_input": { "file_path": file, "old_string": old, "new_string": new }, "tool_response": { "filePath": file } }));
    std::thread::sleep(Duration::from_millis(20));
}

/// The PreToolUse hook of an Edit: what the server prints for it.
fn pre_edit(s: &Server, id: &str, file: &Path, session: &str) -> Value {
    let (status, v) = call_as(s, "POST", "/api/hook", TOKEN, Some(id), Some(json!({ "hook_event_name": "PreToolUse", "tool_name": "Edit", "session_id": session, "tool_input": { "file_path": file, "old_string": "a", "new_string": "b" } })));
    assert_eq!(status, 200, "{v}");
    v["output"].clone()
}

#[test]
fn hotspots_rank_the_file_agents_keep_correcting_and_tell_the_next_agent_once() {
    let s = Server::start(&["--chloectx-bin", "/nonexistent/chloectx"]);
    let (_d, r) = repo();
    std::fs::create_dir(r.join("src")).unwrap();
    let totals = r.join("src/totals.rs");
    let body = "pub fn round_line_total(cents: i64, rate: f64) -> i64 {\n    // floor(cents * rate) lost a cent; round_half_up too\n    let x = (cents as f64 * rate).round() as i64;\n    x\n}\n\npub fn apply_coupon(total: i64, pct: i64) -> i64 {\n    total - total * pct / 100\n}\n";
    std::fs::write(&totals, body).unwrap();
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "totals: round per line"]);
    std::fs::write(&totals, body.replace("round()", "floor()")).unwrap();
    git(&r, &["commit", "-q", "-am", "totals: floor coupons per unit"]);
    git(&r, &["revert", "--no-edit", "HEAD"]);
    let cart = s.spawn(json!({ "repo": r, "name": "cart", "worktree": false }))["id"].as_str().unwrap().to_string();
    let checkout = s.spawn(json!({ "repo": r, "name": "checkout", "worktree": false }))["id"].as_str().unwrap().to_string();
    s.until(&cart, "started", |a| a["session_id"].is_string());
    s.until(&checkout, "started", |a| a["session_id"].is_string());
    let radar = format!("/api/tests?repo={}", r.display());

    // cart changes the rounding, fails the test, puts its edit back, is
    // corrected, and fails again; checkout edits the same file once.
    edit_as(&s, &cart, &totals, "floor", "round_half_up");
    bash_as(&s, &cart, "hu1", "cargo test totals", &cargo_out(false), true);
    s.get_until(&radar, "the first failure", |v| v["totals"]["commands"] == 1);
    edit_as(&s, &cart, &totals, "round_half_up", "floor");
    hook_as(&s, &cart, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "No, don't round per unit: round once, on the line total." }));
    hook_as(&s, &cart, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "Thanks, now add a changelog entry." }));
    edit_as(&s, &cart, &totals, "as f64", "(cents as f64 * rate)");
    edit_as(&s, &cart, &totals, "round", "rate).round()");
    bash_as(&s, &cart, "hu2", "cargo test totals", &cargo_out(false), true);
    s.get_until(&radar, "the second failure", |v| v["totals"]["commands"] == 2);
    edit_as(&s, &checkout, &totals, "pct / 100", "total * pct / 100");
    edit_as(&s, &checkout, &r.join("README"), "hello", "hello, shop");

    let (status, v) = s.post("/api/hotspots/refresh", json!({ "repo": r }));
    assert_eq!(status, 200, "{v}");
    let (status, v) = s.get(&format!("/api/hotspots?repo={}", r.display()));
    assert_eq!(status, 200, "{v}");
    let list = v["hotspots"].as_array().unwrap();
    assert_eq!(list.len(), 1, "README is edited once: not a hotspot: {v}");
    let h = &list[0];
    assert_eq!((h["file"].as_str(), h["repo"].as_str()), (Some("src/totals.rs"), r.to_str()), "{h}");
    let c = &h["counts"];
    assert_eq!((c["edits"].as_i64(), c["re_edits"].as_i64(), c["agents"].as_i64()), (Some(5), Some(3), Some(2)), "{c}");
    // The revert commit, and the edit cart put back.
    assert_eq!(c["reverts"], 2, "{c}");
    assert_eq!((c["test_fails"].as_i64(), c["flaky"].as_i64(), c["steers"].as_i64()), (Some(2), Some(0), Some(1)), "{c}");
    // 3 re-edits + 1 more agent + 2 reverts + 2 failures + 1 correction.
    assert_eq!(h["score"], json!(3.0 + 2.0 + 6.0 + 3.0 + 3.0), "{h}");
    assert_eq!(h["level"], "hot");
    let kinds: Vec<&str> = h["reasons"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds[0], "reverts", "{}", h["reasons"]);
    for k in ["re_edits", "agents", "test_fails", "steers"] {
        assert!(kinds.contains(&k), "{k}: {}", h["reasons"]);
    }
    let tf = h["reasons"].as_array().unwrap().iter().find(|r| r["kind"] == "test_fails").unwrap();
    assert_eq!(tf["text"], "totals::coupon failed twice after edits here", "{tf}");
    // The function the edits fell in, from the edits' lines.
    assert_eq!((h["symbol"].as_str(), h["line"].as_i64()), (Some("round_line_total"), Some(1)), "{}", h["symbols"]);
    assert!(h["summary"].as_str().unwrap().starts_with("agents re-edited round_line_total 3 times"), "{}", h["summary"]);
    // The correction is suggested as a rule: no rule or decision names the file.
    assert_eq!(h["suggestion"]["kind"], "rule", "{}", h["suggestion"]);
    assert_eq!(h["suggestion"]["rule"], "No, don't round per unit: round once, on the line total.");
    assert_eq!(h["churn"].as_array().unwrap().len(), 14);
    assert_eq!(h["churn"].as_array().unwrap().last().unwrap()[1], 5);
    assert_eq!(v["totals"]["hotspots"], 1);
    assert_eq!(v["ctx"]["available"], false, "{}", v["ctx"]);
    assert!(v["agents"].as_array().unwrap().iter().any(|a| a["name"] == "cart"));
    let hid = h["id"].as_str().unwrap().to_string();

    // The evidence, oldest first, with where each piece is.
    let (status, d) = s.get(&format!("/api/hotspots/{hid}"));
    assert_eq!(status, 200, "{d}");
    let ev = d["evidence"].as_array().unwrap();
    let of = |k: &str| ev.iter().filter(|e| e["kind"] == k).count();
    assert_eq!((of("edit"), of("test_fail"), of("prompt"), of("revert")), (5, 2, 1, 2), "{d}");
    assert!(ev.windows(2).all(|w| w[0]["ts_ms"].as_i64() <= w[1]["ts_ms"].as_i64()));
    let fail = ev.iter().find(|e| e["kind"] == "test_fail").unwrap();
    assert_eq!(fail["link"]["test"], "totals::coupon", "{fail}");
    assert!(fail["detail"].as_str().unwrap().contains("left: 1799"), "{fail}");
    assert!(ev.iter().any(|e| e["kind"] == "revert" && e["link"]["commit"].is_string() && e["text"].as_str().unwrap().starts_with("Revert")), "{d}");
    assert!(ev.iter().any(|e| e["kind"] == "revert" && e["link"]["story"]["agent"] == json!(cart)), "{d}");
    assert_eq!(s.get("/api/hotspots/hnosuch").0, 404);

    // The next edit of it in a session gets its history once.
    let out = pre_edit(&s, &checkout, &totals, "sess-1");
    let text = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or_else(|| panic!("{out}"));
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert!(text.starts_with("Workbench: src/totals.rs is a hotspot of this repository (score 17 in 14 days)"), "{text}");
    assert!(text.contains("- totals::coupon failed twice after edits here"), "{text}");
    assert!(text.contains("the user corrected work here: \"No, don't round per unit: round once, on the line total.\""), "{text}");
    assert!(text.contains("ask the user (ask_human)"), "{text}");
    assert_eq!(pre_edit(&s, &checkout, &totals, "sess-1"), Value::Null);
    assert_eq!(pre_edit(&s, &checkout, &r.join("README"), "sess-1"), Value::Null);
    // A new session (after /clear) has forgotten it.
    assert!(pre_edit(&s, &checkout, &totals, "sess-2")["hookSpecificOutput"]["additionalContext"].is_string());
    assert!(s.events(&checkout).iter().any(|e| e["kind"] == "hotspot"));

    // Pin a test and route to a model: the next agent is told both.
    let (status, a) = s.post(&format!("/api/hotspots/{hid}/act"), json!({ "kind": "pin_test", "test": "totals::coupon" }));
    assert_eq!(status, 200, "{a}");
    assert_eq!(a["hotspot"]["pinned_test"], "totals::coupon");
    let (_, a) = s.post(&format!("/api/hotspots/{hid}/act"), json!({ "kind": "route", "model": "opus" }));
    let text = a["hotspot"]["context"].as_str().unwrap();
    assert!(text.contains("run `cargo test totals` first, and again after (the user pinned it)") && text.contains("done by opus"), "{text}");
    assert_eq!(s.post(&format!("/api/hotspots/{hid}/act"), json!({ "kind": "nope" })).0, 400);
    let (_, v) = s.get(&format!("/api/hotspots?repo={}&file=src/totals.rs", r.display()));
    assert_eq!(v["hotspots"][0]["routed_model"], "opus", "{v}");

    // The agents working on one, for Mission's badge.
    let (_, w) = s.get(&format!("/api/hotspots/agents?repo={}", r.display()));
    let names: Vec<&str> = w["agents"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"cart") && names.contains(&"checkout"), "{w}");
    // Another window is scored live.
    let (status, v) = s.get(&format!("/api/hotspots?repo={}&days=1", r.display()));
    assert_eq!((status, v["window"]["days"].as_i64()), (200, Some(1)), "{v}");
    assert_eq!(v["hotspots"][0]["file"], "src/totals.rs");

    // Agents ask through MCP.
    let mut m = Mcp::start(&s, &cart);
    m.send("initialize", json!({}));
    m.notify("notifications/initialized");
    let (text, err) = m.tool("hotspots", json!({}));
    assert!(!err && text.contains("src/totals.rs › round_line_total [hot 17.0]"), "{text}");
    let (text, err) = m.tool("hotspots", json!({ "file": "src/totals.rs" }));
    assert!(!err && text.starts_with("Workbench: src/totals.rs is a hotspot") && text.contains("totals::coupon (2 runs, 2 failed)"), "{text}");
    let (text, err) = m.tool("hotspots", json!({ "file": "README" }));
    assert!(!err && text.contains("README is not a hotspot"), "{text}");
}
