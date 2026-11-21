//! `chloectx forget --everywhere`: one command finds a fact in every store
//! `chloectx stores` lists, by its text (whatever the case) or by record
//! id, lists what goes and what stays, asks first (or takes --yes;
//! --dry-run only lists), and forgets it with each store's own forget.
//! A record whose own text holds it goes; one that holds it only in its
//! context (the turn's request) stays, with it cut from there (JONAH-1).
//! `chloectx forget TEXT` does the same in this project's store. The real
//! program, with stores in a home of the test's own and the hash embedder.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Homes {
    t: tempfile::TempDir,
}

impl Homes {
    fn new() -> Homes {
        let t = tempfile::tempdir().unwrap();
        for d in ["ctx", "a", "b", "c"] {
            std::fs::create_dir_all(t.path().join(d)).unwrap();
        }
        Homes { t }
    }

    fn project(&self, p: &str) -> PathBuf {
        self.t.path().join(p).canonicalize().unwrap()
    }

    fn run(&self, project: &str, args: &[&str]) -> (i32, String, String) {
        self.run_with(project, args, "")
    }

    fn run_with(&self, project: &str, args: &[&str], input: &str) -> (i32, String, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_chloectx"))
            .args(args)
            .current_dir(self.project(project))
            .env("HOME", self.t.path())
            .env("CHLOECTX_HOME", self.t.path().join("ctx"))
            .env("CHLOECTX_EMBEDDER", "hash:64")
            .env("CHLOECTX_DISTILL", "0")
            .env("CHLOECTX_DEEP", "off")
            .env("CHLOECTX_RECEIPTS", "0")
            .env_remove("CLAUDE_PROJECT_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    /// A hook, as Claude Code runs it in the project's folder.
    fn hook(&self, project: &str, event: &str, payload: serde_json::Value) {
        let (code, out, err) = self.run_with(project, &["hook", event], &payload.to_string());
        assert_eq!(code, 0, "{out}{err}");
    }

    fn remember(&self, project: &str, text: &str) -> String {
        let (code, out, err) = self.run(project, &["remember", "fact", text]);
        assert_eq!(code, 0, "{out}{err}");
        // "Stored fact as <id> in ChloeCtx (<store>), where every later session sees it."
        out.split("Stored fact as ").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or_else(|| panic!("no id in {out}")).trim_end_matches('.').to_string()
    }

    fn count(&self, project: &str, needle: &str) -> usize {
        self.count_where(project, &format!("text LIKE '%{needle}%'"))
    }

    fn count_where(&self, project: &str, cond: &str) -> usize {
        let (_, out, _) = self.run(project, &["sql", &format!("SELECT id FROM memory WHERE {cond}")]);
        out.lines().filter(|l| l.contains(':') && !l.contains("id")).count()
    }
}

impl Drop for Homes {
    fn drop(&mut self) {
        let _ = self.run("a", &["stop", "--all"]);
    }
}

#[test]
fn a_fact_is_found_in_every_store_and_forgotten_once_asked() {
    let h = Homes::new();
    h.remember("a", "The staging codeword is Zebra-Quartz-77, changed monthly");
    h.remember("a", "Unrelated: the build uses cargo nextest");
    h.remember("b", "The demo banner still shows zebra-quartz-77 too");
    let keep = h.remember("c", "Nothing to see in project c");

    // Too short to look for.
    assert_eq!(h.run("a", &["forget", "--everywhere", "ze"]).0, 2);
    // Listed, whatever the case, in both stores; nothing changed.
    let (code, out, err) = h.run("c", &["forget", "--everywhere", "--dry-run", "ZEBRA-quartz-77"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains(&h.project("a").display().to_string()) && out.contains(&h.project("b").display().to_string()), "{out}");
    assert!(!out.contains(&format!("{} (", h.project("c").display())), "c holds no match: {out}");
    assert!(out.contains("Would forget 2 records in 2 stores. Nothing was changed."), "{out}");
    assert_eq!(h.count("a", "Zebra"), 1);
    // Without a terminal and without --yes it does not ask, and changes nothing.
    let (_, out, _) = h.run("c", &["forget", "--everywhere", "zebra-quartz-77"]);
    assert!(out.contains("Run it again with --yes"), "{out}");
    assert_eq!(h.count("b", "zebra"), 1);

    let (code, out, err) = h.run("c", &["forget", "--everywhere", "--yes", "zebra-quartz-77"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("Forgot 2 records."), "{out}");
    assert_eq!(h.count("a", "Zebra"), 0);
    assert_eq!(h.count("b", "zebra"), 0);
    assert_eq!(h.count("a", "nextest"), 1, "the other records stay");
    let (_, out, _) = h.run("a", &["forget", "--everywhere", "--dry-run", "zebra-quartz-77"]);
    assert!(out.contains("No record in any of the stores holds"), "{out}");

    // By id, wherever its store is.
    let (code, out, _) = h.run("a", &["forget", "--everywhere", "--yes", &keep]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("Deleted {keep}")), "{out}");
    assert_eq!(h.count("c", "project c"), 0);
    assert_eq!(h.count("a", "nextest"), 1, "ids repeat across stores; only the one that exists went");
    // Every store is still there; none was created for a folder without one.
    let (_, out, _) = h.run("a", &["stores"]);
    assert!(out.contains("\n3 stores, "), "{out}");
    assert!(Path::new(env!("CARGO_BIN_EXE_chloectx")).exists());
}

/// JONAH-1: a private aside and a rule in one prompt. Forgetting the aside
/// deletes the records whose own text holds it, keeps the ones that hold
/// it only in their context (the turn's request) with it cut from there,
/// and lists each record that goes, with its kind and first line, before
/// it asks.
#[test]
fn forgetting_an_aside_keeps_what_was_said_with_it() {
    let h = Homes::new();
    let a = h.project("a");
    let p = a.to_str().unwrap();
    h.hook("a", "SessionStart", serde_json::json!({"session_id": "s1", "source": "startup", "cwd": p}));
    h.hook("a", "UserPromptSubmit", serde_json::json!({"session_id": "s1", "cwd": p, "prompt": "I'll sell tdo later as part of the Walrus-Bundle. From now on, always write the tests with pytest, never with unittest."}));
    h.hook("a", "Stop", serde_json::json!({"session_id": "s1", "cwd": p, "last_assistant_message": "Understood: every test uses pytest."}));
    let keep = h.remember("a", "The CLI parses its arguments with argparse");
    let answers = h.count_where("a", "kind = 'answer'");
    assert_eq!(answers, 1);
    assert!(h.count_where("a", "kind = 'answer' AND MATCH(body, 'walrus')") == 1, "the answer holds it in its context");

    let (code, out, err) = h.run("b", &["forget", "--everywhere", "--dry-run", "walrus-bundle"]);
    assert_eq!(code, 0, "{out}{err}");
    let deletes = out.split("Keeps").next().unwrap();
    assert!(deletes.contains("whose own text holds it:") && deletes.contains(" prompt ") && deletes.contains("I'll sell tdo later as part of the Walrus-Bundle"), "{out}");
    assert!(!deletes.contains(" answer "), "the answer does not go: {out}");
    let keeps = out.split("Keeps").nth(1).unwrap_or_else(|| panic!("{out}"));
    assert!(keeps.contains("only in its context") && keeps.contains(" answer ") && keeps.contains("Understood: every test uses pytest."), "{out}");
    assert!(out.contains("Scrubs it from 1 session's state"), "{out}");
    assert!(out.contains("Nothing was changed."), "{out}");

    // In the project's own store, without --everywhere: listed, not asked
    // without a terminal.
    let (code, out, _) = h.run("a", &["forget", "Walrus-Bundle"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Deletes") && out.contains("Run it again with --yes"), "{out}");

    let (code, out, err) = h.run("a", &["forget", "--yes", "WALRUS-bundle"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("which stay"), "{out}");
    assert_eq!(h.count_where("a", "kind = 'answer'"), 1, "the answer stays");
    assert_eq!(h.count_where("a", "MATCH(body, 'walrus')"), 0, "and holds it no more, in its text or its context");
    assert_eq!(h.count("a", "argparse"), 1);
    assert!(h.count_where("a", &format!("id = '{keep}'")) == 1);
    let dir = h.t.path().join("ctx/projects");
    let store = std::fs::read_dir(&dir).unwrap().flatten().next().unwrap().path();
    let state = std::fs::read_to_string(store.join("state.json")).unwrap();
    assert!(!state.to_lowercase().contains("walrus"), "{state}");
    let (_, out, _) = h.run("a", &["forget", "--everywhere", "--dry-run", "walrus-bundle"]);
    assert!(out.contains("No record in any of the stores holds"), "{out}");
}

/// ANA-303: `chloectx forget --yes ... | head -1`. A reader that stops
/// reading (here, stdout closed before it starts) changes nothing of what
/// it does: the fact is forgotten, by text and by id, and it exits as it
/// would have.
#[test]
fn forget_forgets_whoever_reads_what_it_says() {
    let h = Homes::new();
    h.remember("a", "The vault phrase is Otter-Lantern-31 this week");
    let id = h.remember("a", "The second vault phrase is Heron-Kettle-58");
    h.remember("a", "The build uses cargo nextest");
    let closed = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_chloectx"))
            .args(args)
            .current_dir(h.project("a"))
            .env("HOME", h.t.path())
            .env("CHLOECTX_HOME", h.t.path().join("ctx"))
            .env("CHLOECTX_EMBEDDER", "hash:64")
            .env("CHLOECTX_DISTILL", "0")
            .env("CHLOECTX_DEEP", "off")
            .env("CHLOECTX_RECEIPTS", "0")
            .env_remove("CLAUDE_PROJECT_DIR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The reader goes away before the first line.
        let mut child = out;
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (code, err) = closed(&["forget", "--yes", "otter-lantern-31"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(!err.contains("panicked"), "{err}");
    assert_eq!(h.count("a", "Otter"), 0, "forgotten though nobody read it");
    let (code, err) = closed(&["forget", &id]);
    assert_eq!(code, Some(0), "{err}");
    assert!(!err.contains("panicked"), "{err}");
    assert_eq!(h.count("a", "Heron"), 0);
    assert_eq!(h.count("a", "nextest"), 1);
}
