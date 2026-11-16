//! A request that reaches the daemon while it shuts down is answered (by
//! the next daemon), not dropped by the exit. `chloectx sync setup` stops
//! the project's daemon, and a `chloectx sync push` right after it read
//! nothing: "chloectx: daemon: EOF while parsing a value at line 1 column
//! 0". The real program, with a store in a home of the test's own, the
//! hash embedder, and a sync server that refuses connections (the
//! shutdown's last push fails at once, as a 403 does).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn run(home: &Path, project: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_chloectx"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env("CHLOECTX_HOME", home.join("ctx"))
        .env("CHLOECTX_EMBEDDER", "hash:64")
        .env("CHLOECTX_DISTILL", "0")
        .env("CHLOECTX_DEEP", "off")
        .env("CHLOECTX_RECEIPTS", "0")
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// A local address nothing listens on.
fn refused() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}")
}

struct Stop(PathBuf, PathBuf);

impl Drop for Stop {
    fn drop(&mut self) {
        let _ = run(&self.0, &self.1, &["stop", "--all"]);
    }
}

#[test]
fn a_request_right_after_a_daemon_was_told_to_stop_is_answered() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().canonicalize().unwrap();
    let project = home.join("p");
    std::fs::create_dir_all(&project).unwrap();
    let _stop = Stop(home.clone(), project.clone());
    let url = refused();
    let (code, out, err) = run(&home, &project, &["remember", "fact", "The kiln runs at cone 6 on Thursdays"]);
    assert_eq!(code, 0, "{out}{err}");
    for round in 0..5 {
        // Stops the running daemon (its answer comes first, then the exit).
        let (code, out, err) = run(&home, &project, &["sync", "setup", "--url", &url, "--no-create"]);
        assert_eq!(code, 0, "round {round}: {out}{err}");
        assert!(out.contains("sync set up with"), "round {round}: {out}{err}");
        // Reaches it as it exits, or the next one.
        let (code, out, err) = run(&home, &project, &["sync", "push"]);
        assert_eq!(code, 0, "round {round}: {out}{err}");
        assert!(!err.contains("EOF while parsing"), "round {round}: {err}");
        let r: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("round {round}: {e}: {out}{err}"));
        assert_eq!(r["server"].as_str(), Some(url.as_str()), "round {round}: {out}");
        assert!(r["local_records"].as_u64().is_some_and(|n| n >= 1), "round {round}: {out}");
        // And a read, through the same restart.
        let (code, out, err) = run(&home, &project, &["sync", "setup", "--url", &url, "--no-create"]);
        assert_eq!(code, 0, "round {round}: {out}{err}");
        let (code, out, err) = run(&home, &project, &["sql", "SELECT id FROM memory WHERE text LIKE '%kiln%'"]);
        assert_eq!(code, 0, "round {round}: {out}{err}");
        assert!(out.contains("(1 rows)"), "round {round}: {out}{err}");
    }
}
