//! `llm::complete` against a stand-in for the `claude` CLI: a shell script
//! that writes down where and how it was run and answers as `claude -p
//! --output-format json` does.

#![cfg(unix)]

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chloectx::llm::{complete, Cli};

/// ChloeCtx's home for this test binary (set once, before any call reads it).
fn home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let d = tempfile::tempdir().unwrap().keep();
        std::env::set_var("CHLOECTX_HOME", &d);
        d
    })
}

/// A fake CLI in `dir` that records its working directory and arguments
/// in `dir`, then prints `reply_bytes` bytes of answer.
fn fake_cli(dir: &Path, reply_bytes: usize) -> PathBuf {
    let exe = dir.join("claude");
    let script = format!(
        "#!/bin/sh\npwd > '{d}/cwd'\nprintf '%s\\n' \"$@\" > '{d}/args'\ncat > /dev/null\nprintf '{{\"is_error\": false, \"result\": \"'\nhead -c {reply_bytes} /dev/zero | tr '\\0' 'x'\nprintf '\", \"total_cost_usd\": 0.001}}'\n",
        d = dir.display()
    );
    std::fs::write(&exe, script).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

fn cli(exe: &Path, timeout: Duration) -> Cli<'_> {
    Cli { command: exe.to_str().unwrap(), model: "", timeout, isolated: true }
}

/// B-04: the CLI loads project settings and CLAUDE.md from its working
/// directory, so that directory must be private to this user, never a
/// shared, predictable one in /tmp.
#[test]
fn the_model_runs_in_a_private_directory() {
    home();
    let t = tempfile::tempdir().unwrap();
    let exe = fake_cli(t.path(), 10);
    let r = complete(&cli(&exe, Duration::from_secs(20)), "system", "prompt").unwrap();
    assert_eq!(r.text, "x".repeat(10));
    let cwd = PathBuf::from(std::fs::read_to_string(t.path().join("cwd")).unwrap().trim());
    assert!(!cwd.starts_with(std::env::temp_dir().join("chloectx-distill")), "ran in the shared {}", cwd.display());
    assert!(cwd.canonicalize().unwrap().starts_with(home().canonicalize().unwrap()), "ran in {}", cwd.display());
    let m = std::fs::symlink_metadata(&cwd).unwrap();
    assert!(m.is_dir());
    assert_eq!(m.uid(), unsafe { libc::getuid() });
    assert_eq!(m.mode() & 0o077, 0, "mode {:o}", m.mode() & 0o777);
    // Still only the (empty) project's settings, and no MCP servers.
    let args = std::fs::read_to_string(t.path().join("args")).unwrap();
    assert!(args.contains("--setting-sources\nproject\n") && args.contains("--strict-mcp-config"), "{args}");

    // Left open (by an older version, or by hand): narrowed again before
    // the next call. (In this test: the others share the directory.)
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o777)).unwrap();
    complete(&cli(&exe, Duration::from_secs(20)), "s", "p").unwrap();
    assert_eq!(std::fs::metadata(&cwd).unwrap().mode() & 0o777, 0o700);
}

/// B-17: an answer larger than a pipe's buffer (64 KB on Linux) comes back,
/// rather than blocking the CLI on its write until the timeout kills it.
#[test]
fn a_long_answer_is_read_while_the_cli_writes_it() {
    home();
    let t = tempfile::tempdir().unwrap();
    let exe = fake_cli(t.path(), 300_000);
    let t0 = Instant::now();
    let r = complete(&cli(&exe, Duration::from_secs(8)), "system", &"a long prompt ".repeat(20_000)).unwrap();
    assert_eq!(r.text.len(), 300_000);
    assert!(t0.elapsed() < Duration::from_secs(8), "{:?}", t0.elapsed());
}

/// The timeout still holds for a CLI that never answers.
#[test]
fn a_cli_that_hangs_times_out() {
    home();
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("claude");
    std::fs::write(&exe, "#!/bin/sh\nsleep 30\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let t0 = Instant::now();
    let e = complete(&cli(&exe, Duration::from_secs(1)), "s", "p").unwrap_err();
    assert!(e.to_string().contains("timed out"), "{e}");
    assert!(t0.elapsed() < Duration::from_secs(10));
}
