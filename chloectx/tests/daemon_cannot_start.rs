//! A daemon that cannot start (ANA-501): its embedding model is gone. The
//! hooks answer within their budgets and at once after the first, say so
//! once a session with the daemon's own words, and the next try waits for
//! a while. The real program, with a store in a home of the test's own and
//! a models folder that is empty.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn hook(home: &Path, project: &Path, event: &str, session: &str) -> (Duration, String, String) {
    let t = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_chloectx"))
        .args(["hook", event])
        .current_dir(project)
        .env("HOME", home)
        .env("CHLOECTX_HOME", home.join("ctx"))
        .env("CHLOECTX_MODELS", home.join("no-models"))
        .env("CHLOECTX_EMBEDDER", "static")
        .env("CHLOECTX_DISTILL", "0")
        .env("CHLOECTX_DEEP", "off")
        .env("CHLOECTX_RECEIPTS", "0")
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("CHLOECTX_DISABLE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = serde_json::json!({"session_id": session, "hook_event_name": event, "cwd": project, "prompt": "hello", "source": "startup"});
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "a hook never fails");
    (t.elapsed(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        }
    }
    None
}

#[test]
fn a_daemon_that_cannot_start_is_said_once_and_costs_no_wait() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let project = home.join("p");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(home.join("no-models")).unwrap();

    // The first hook starts the daemon, which exits: it is said in the
    // session, with why, well within SessionStart's 8 s.
    let (took, out, err) = hook(&home, &project, "SessionStart", "s1");
    assert!(took < Duration::from_secs(5), "{took:?}");
    let said: serde_json::Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{e}: {out}{err}"));
    let msg = said["systemMessage"].as_str().unwrap();
    assert!(msg.contains("could not start its daemon") && msg.contains("chloectx doctor"), "{msg}");
    assert!(err.contains("the daemon could not start") && !err.contains("did not answer in time"), "{err}");
    let mark = find(&home.join("ctx"), "start-failed.json").expect("start-failed.json");
    assert_eq!(std::fs::metadata(&mark).unwrap().permissions().mode() & 0o777, 0o600);

    // The next hooks of the session answer at once, and say nothing more.
    for event in ["UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"] {
        let (took, out, err) = hook(&home, &project, event, "s1");
        assert!(took < Duration::from_millis(1000), "{event}: {took:?}");
        assert!(out.trim().is_empty(), "{event}: told once: {out}");
        assert!(err.contains("could not start"), "{event}: {err}");
    }
    // Another session is told too.
    let (_, out, _) = hook(&home, &project, "UserPromptSubmit", "s2");
    assert!(out.contains("could not start its daemon"), "{out}");
}

use std::os::unix::fs::PermissionsExt;
