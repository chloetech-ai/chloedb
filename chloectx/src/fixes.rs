//! Error to fix. A command that failed and then passed, with edits or other
//! commands in between, is written down as a fix: an `error` record tagged
//! `fix` that holds the error, the steps and the command that then passed.
//! When a command fails again with the same error, in any session of the
//! project, the fix is shown to the model with the failure
//! ([`crate::daemon`], `PostToolUse` and `PostToolUseFailure`), so it can
//! repeat what worked instead of finding it again.
//!
//! A failure is followed for the next [`OPEN_FOR`] tool calls of its session.
//! It is fixed when the same command (after [`command_key`]) succeeds, or
//! fails with a different error: the steps since it last failed with that
//! error, at most [`MAX_STEPS`], are the fix. A command that passes on its
//! own, with no steps between, is not a fix.
//!
//! An error is matched by its signature ([`signature`]): its telling line,
//! with numbers, hashes, temporary paths and the project's path masked.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::capture::{looks_like_error, response_text};
use crate::record::{Draft, Kind, Record, COLUMNS};
use crate::redact::redact;
use crate::store::{row_record, Store};
use crate::Result;

/// The tag of a fix's record.
pub const TAG: &str = "fix";
/// Steps a fix keeps, the last ones.
pub const MAX_STEPS: usize = 8;
/// Failures followed at once in a session.
pub const MAX_OPEN: usize = 4;
/// Tool calls a failure is followed for.
pub const OPEN_FOR: usize = 60;

/// The failures a session is following.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tracker {
    open: Vec<Open>,
    calls: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Open {
    sig: String,
    /// The error's line, as the output had it (redacted).
    line: String,
    command: String,
    key: String,
    since: usize,
    steps: Vec<String>,
    files: Vec<String>,
}

/// A failure that a later call cleared.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixed {
    pub sig: String,
    pub line: String,
    /// The command that failed and then passed.
    pub command: String,
    pub steps: Vec<String>,
    pub files: Vec<String>,
    /// What showed it fixed: the command passed, or failed with another error.
    pub then: String,
}

impl Fixed {
    /// The fix's record: the error, the steps, and what showed it fixed; the
    /// signature in its context, which the lookup reads.
    pub fn draft(&self, ctx: &str) -> Draft {
        let mut t = format!("Error: {}\nFrom: $ {}\nCleared by:", self.line, self.command);
        for s in &self.steps {
            t += &format!("\n- {s}");
        }
        t += &format!("\n{}", self.then);
        let ctx = if ctx.is_empty() { format!("Signature: {}", self.sig) } else { format!("Signature: {}\n{ctx}", self.sig) };
        Draft::new(Kind::Error, t).tag(TAG).files(self.files.clone()).ctx(ctx).importance(0.8)
    }
}

impl Tracker {
    /// A tool's call and its result: the failures it shows fixed.
    pub fn observe(&mut self, tool: &str, input: &Json, resp: &Json, project: &str) -> Vec<Fixed> {
        self.calls += 1;
        let now = self.calls;
        self.open.retain(|o| now - o.since <= OPEN_FOR);
        let text = response_text(resp);
        let failed = looks_like_error(resp, &text);
        let s = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let mut fixed = Vec::new();
        match tool {
            "Bash" => {
                let command = s("command");
                let key = command_key(command);
                if key.is_empty() {
                    return fixed;
                }
                if failed {
                    let Some((sig, line)) = signature(&text, project) else { return fixed };
                    match self.open.iter().position(|o| o.key == key) {
                        Some(i) if self.open[i].sig == sig => {
                            // The steps since did not clear it.
                            let o = &mut self.open[i];
                            o.steps.clear();
                            o.files.clear();
                            o.since = now;
                        }
                        Some(i) => {
                            let o = self.open.remove(i);
                            let then = format!("Then the same command failed with another error: {}", clip(&line, 200));
                            fixed.extend(o.fixed(then));
                            self.push(Open { sig, line, command: clip(&redact(command), 300), key, since: now, steps: Vec::new(), files: Vec::new() });
                        }
                        None => self.push(Open { sig, line, command: clip(&redact(command), 300), key, since: now, steps: Vec::new(), files: Vec::new() }),
                    }
                } else {
                    if let Some(i) = self.open.iter().position(|o| o.key == key) {
                        let o = self.open.remove(i);
                        fixed.extend(o.fixed("Then the same command passed.".to_string()));
                    }
                    if changes_things(&key) {
                        let step = format!("$ {}", clip(&redact(command.trim()), 300));
                        for o in &mut self.open {
                            o.step(step.clone(), None);
                        }
                    }
                }
            }
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" if !failed => {
                let path = [s("file_path"), s("notebook_path")].into_iter().find(|p| !p.is_empty()).unwrap_or("");
                let path = path.strip_prefix(project).map(|p| p.trim_start_matches('/')).unwrap_or(path).to_string();
                let step = match tool {
                    "Write" => format!("Wrote {path}"),
                    "NotebookEdit" => format!("Edited the notebook {path}"),
                    _ => {
                        let edits: Vec<(&str, &str)> = match input.get("edits").and_then(|e| e.as_array()) {
                            Some(list) => list.iter().map(|e| (e.get("old_string").and_then(|v| v.as_str()).unwrap_or(""), e.get("new_string").and_then(|v| v.as_str()).unwrap_or(""))).collect(),
                            None => vec![(s("old_string"), s("new_string"))],
                        };
                        let shown: Vec<String> = edits.iter().take(2).map(|(o, n)| format!("`{}` -> `{}`", one_line(o, 120), one_line(n, 240))).collect();
                        let more = if edits.len() > 2 { format!(" (and {} more)", edits.len() - 2) } else { String::new() };
                        redact(&format!("Edited {path}: {}{more}", shown.join("; ")))
                    }
                };
                for o in &mut self.open {
                    o.step(step.clone(), Some(&path));
                }
            }
            _ => {}
        }
        fixed
    }

    fn push(&mut self, o: Open) {
        if self.open.len() >= MAX_OPEN {
            self.open.remove(0);
        }
        self.open.push(o);
    }

    /// Failures followed now (for tests and the inspector).
    pub fn open(&self) -> usize {
        self.open.len()
    }
}

impl Open {
    fn step(&mut self, step: String, file: Option<&str>) {
        if self.steps.last() == Some(&step) {
            return;
        }
        self.steps.push(step);
        if self.steps.len() > MAX_STEPS {
            self.steps.remove(0);
        }
        if let Some(f) = file.filter(|f| !f.is_empty()) {
            if !self.files.iter().any(|x| x == f) {
                self.files.push(f.to_string());
            }
        }
    }

    fn fixed(self, then: String) -> Option<Fixed> {
        if self.steps.is_empty() {
            return None;
        }
        Some(Fixed { sig: self.sig, line: self.line, command: self.command, steps: self.steps, files: self.files, then })
    }
}

/// A command as matched against the same command run again: the last of
/// its `&&`, `||` and `;` parts that is not a `cd`, `export` or `source`,
/// before any pipe, without redirections, leading variable assignments,
/// `timeout N`, `time` or `env`, and its spaces collapsed.
pub fn command_key(command: &str) -> String {
    let one = command.split_whitespace().collect::<Vec<_>>().join(" ");
    let parts: Vec<&str> = one.split("&&").flat_map(|p| p.split("||")).flat_map(|p| p.split(';')).map(str::trim).filter(|p| !p.is_empty()).collect();
    let main = parts.iter().rev().find(|p| {
        let first = p.split_whitespace().next().unwrap_or("");
        !matches!(first, "cd" | "pushd" | "popd" | "export" | "source" | "." | "set" | "unset" | "true" | "echo")
    });
    let Some(main) = main else { return String::new() };
    let before_pipe = main.split('|').next().unwrap_or("");
    // Redirections go, with the file a bare operator names (`> out.txt`).
    let mut toks: Vec<&str> = Vec::new();
    let mut words = before_pipe.split_whitespace();
    while let Some(t) = words.next() {
        if t.contains('>') || t.starts_with('<') {
            if t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&').trim_start_matches(['>', '<']).is_empty() {
                words.next();
            }
            continue;
        }
        toks.push(t);
    }
    loop {
        match toks.first() {
            Some(t) if t.contains('=') && !t.starts_with('-') && t.split('=').next().is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) => {
                toks.remove(0);
            }
            Some(&"time") | Some(&"env") | Some(&"nice") | Some(&"command") => {
                toks.remove(0);
            }
            Some(&"timeout") => {
                toks.remove(0);
                while let Some(flag) = toks.first().filter(|t| t.starts_with('-')).copied() {
                    toks.remove(0);
                    // -s SIGNAL and -k DURATION take a value.
                    if matches!(flag, "-s" | "-k" | "--signal" | "--kill-after") && !toks.is_empty() {
                        toks.remove(0);
                    }
                }
                if !toks.is_empty() {
                    toks.remove(0);
                }
            }
            _ => break,
        }
    }
    toks.join(" ")
}

/// Whether a command (its key) may change what the next run sees: not one
/// that only looks (`ls`, `cat`, `grep`, `git status`, ...).
fn changes_things(key: &str) -> bool {
    let mut w = key.split_whitespace();
    let first = w.next().unwrap_or("");
    let first = first.rsplit('/').next().unwrap_or(first);
    match first {
        "ls" | "cat" | "head" | "tail" | "less" | "more" | "grep" | "rg" | "ag" | "find" | "fd" | "pwd" | "wc" | "which" | "type" | "file" | "tree" | "du" | "df" | "stat" | "printenv" | "date" | "whoami" | "uname" | "sleep" | "diff" | "jq" | "awk" => false,
        "sed" => key.contains(" -i"),
        "git" => !matches!(w.next().unwrap_or(""), "status" | "diff" | "log" | "show" | "blame" | "branch" | "remote" | "rev-parse" | "ls-files" | "grep"),
        _ => true,
    }
}

/// An error's signature, and its line as the output had it: the line that
/// tells the error (a compiler's `error:`, the last line of a Python
/// traceback, a panic, `FAILED`, `fatal:`, ...), or the first line, with the
/// project's path, temporary paths, numbers and hashes masked. None for an
/// empty output.
pub fn signature(text: &str, project: &str) -> Option<(String, String)> {
    static RES: OnceLock<[Regex; 2]> = OnceLock::new();
    let [strong, weak] = RES.get_or_init(|| {
        [
            Regex::new(r"(^error(\[E\d+\])?:|\bpanicked at\b|^fatal:|^\w*(Error|Exception)\b.*:|\bcommand not found\b|\bNo such file or directory\b|^npm ERR!|^ERROR\b|^E\s{2,}\w+Error|\bcannot find\b|\bnot found\b|\bundefined reference\b|\bModuleNotFoundError\b)").unwrap(),
            Regex::new(r"(?i)(\berror\b|\bfailed\b|\bfailure\b|\bdenied\b|\brefused\b|\binvalid\b|\bunknown\b)").unwrap(),
        ]
    });
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("[stderr]")).collect();
    if lines.is_empty() {
        return None;
    }
    // A traceback's error is its last line; other tools say it first.
    let line = if text.contains("Traceback (most recent call last)") {
        lines.iter().rev().find(|l| strong.is_match(l)).copied()
    } else {
        lines.iter().find(|l| strong.is_match(l) && !l.starts_with("test result:")).copied()
    }
    .or_else(|| lines.iter().find(|l| weak.is_match(l)).copied())
    .unwrap_or(lines[0]);
    let shown = clip(&redact(line), 300);
    let sig = mask(line, project);
    (!sig.is_empty()).then_some((sig, shown))
}

/// `line` with what changes from run to run masked.
fn mask(line: &str, project: &str) -> String {
    static RES: OnceLock<[Regex; 4]> = OnceLock::new();
    let [tmp, hex, num, space] = RES.get_or_init(|| {
        [
            Regex::new(r"(/private)?/(tmp|var/folders)/[^\s:'`]*").unwrap(),
            Regex::new(r"\b(0x[0-9a-fA-F]+|[0-9a-f]{7,})\b").unwrap(),
            Regex::new(r"\d+").unwrap(),
            Regex::new(r"\s+").unwrap(),
        ]
    });
    let mut s = line.to_string();
    if !project.is_empty() {
        s = s.replace(&format!("{project}/"), "").replace(project, ".");
    }
    let s = tmp.replace_all(&s, "<tmp>");
    let s = hex.replace_all(&s, "#");
    let s = num.replace_all(&s, "#");
    let s = space.replace_all(s.trim(), " ");
    clip(&s, 200)
}

/// The fixes written down for an error, best first: those of the same
/// signature, newest first, then those whose signatures share most of its
/// words (at least four in five).
pub fn known(store: &Store, sig: &str, limit: usize) -> Result<Vec<Record>> {
    let q = sig.replace('#', " ");
    let sql = format!("SELECT {COLUMNS} FROM memory WHERE kind = 'error' AND tag = '{TAG}' ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 30");
    let rs = store.query_with(&sql, &chloedb::query::Params::new().set("q", chloedb::Value::Str(q)))?;
    let want = words(sig);
    let mut found: Vec<(bool, f64, Record)> = rs
        .rows
        .iter()
        .filter_map(|r| row_record(r))
        .filter_map(|r| {
            let theirs = r.ctx.lines().find_map(|l| l.strip_prefix("Signature: "))?.to_string();
            if theirs == sig {
                return Some((true, 1.0, r));
            }
            let t = words(&theirs);
            let inter = want.iter().filter(|w| t.contains(*w)).count() as f64;
            let union = (want.len() + t.len()) as f64 - inter;
            let j = if union > 0.0 { inter / union } else { 0.0 };
            (j >= 0.8).then_some((false, j, r))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)).then(b.2.ts.cmp(&a.2.ts)));
    Ok(found.into_iter().take(limit).map(|x| x.2).collect())
}

fn words(s: &str) -> Vec<String> {
    let mut w: Vec<String> = s.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| w.len() > 1).map(|w| w.to_lowercase()).collect();
    w.sort();
    w.dedup();
    w
}

/// The context shown with a failure that has known fixes.
pub fn render(fixes: &[Record]) -> String {
    if fixes.is_empty() {
        return String::new();
    }
    let mut t = String::from("ChloeCtx: this error was seen before, and cleared as follows. Check that the steps fit the code as it is now before repeating them.");
    for r in fixes {
        let when = crate::recall::fmt_time(r.ts);
        t += &format!("\n\n[{} {when}, session {}, turn {}]\n{}", r.id, short(&r.session), r.turn, r.text);
    }
    t
}

fn short(s: &str) -> &str {
    &s[..s.char_indices().nth(8).map_or(s.len(), |(i, _)| i)]
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn one_line(s: &str, max: usize) -> String {
    clip(&s.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const P: &str = "/home/u/proj";

    fn bash(cmd: &str, out: &str, code: i64) -> (Json, Json) {
        (json!({"command": cmd}), json!({"stdout": out, "exit_code": code}))
    }

    #[test]
    fn keys_ignore_cd_pipes_redirections_and_timeouts() {
        assert_eq!(command_key("cd /x && cargo test --test mcp 2>&1 | tail -20"), "cargo test --test mcp");
        assert_eq!(command_key("RUST_LOG=debug timeout 600 cargo   build"), "cargo build");
        assert_eq!(command_key("timeout -s KILL 60 pytest tests/test_a.py > out.txt"), "pytest tests/test_a.py");
        assert_eq!(command_key("cd /x; export A=1; npm test"), "npm test");
        assert_eq!(command_key("cd /x"), "");
    }

    #[test]
    fn signatures_mask_what_changes_between_runs() {
        let rust = "   Compiling x v0.1.0\nerror[E0425]: cannot find value `foo` in this scope\n --> /home/u/proj/src/lib.rs:12:5\n";
        let (sig, line) = signature(rust, P).unwrap();
        assert_eq!(sig, "error[E#]: cannot find value `foo` in this scope");
        assert_eq!(line, "error[E0425]: cannot find value `foo` in this scope");
        let py = "Traceback (most recent call last):\n  File \"/home/u/proj/a.py\", line 3, in <module>\n    import numpy\nModuleNotFoundError: No module named 'numpy'\n";
        assert_eq!(signature(py, P).unwrap().0, "ModuleNotFoundError: No module named 'numpy'");
        let panic = "running 3 tests\nthread 'tests::it_works' panicked at /home/u/proj/src/a.rs:40:9:\nassertion failed\ntest result: FAILED. 2 passed; 1 failed";
        assert_eq!(signature(panic, P).unwrap().0, "thread 'tests::it_works' panicked at src/a.rs:#:#:");
        let tmp = "fatal: could not open /tmp/.tmpAb12Cd/db.strata: busy";
        assert_eq!(signature(tmp, P).unwrap().0, "fatal: could not open <tmp>: busy");
        assert!(signature("", P).is_none());
    }

    #[test]
    fn a_failure_cleared_by_edits_is_a_fix_and_one_that_passes_alone_is_not() {
        let mut t = Tracker::default();
        let (i, r) = bash("cd /home/u/proj && cargo test 2>&1 | tail", "error[E0425]: cannot find value `foo` in this scope", 101);
        assert!(t.observe("Bash", &i, &r, P).is_empty());
        assert_eq!(t.open(), 1);
        // Looking changes nothing; an edit and a command that changes things are steps.
        let (i2, r2) = bash("grep -n foo src/lib.rs", "12: foo", 0);
        assert!(t.observe("Bash", &i2, &r2, P).is_empty());
        let edit = json!({"file_path": "/home/u/proj/src/lib.rs", "old_string": "foo", "new_string": "let foo = 1;\nfoo"});
        assert!(t.observe("Edit", &edit, &json!({}), P).is_empty());
        let (i3, r3) = bash("cargo fmt", "", 0);
        assert!(t.observe("Bash", &i3, &r3, P).is_empty());
        let (i4, r4) = bash("cargo test", "test result: ok. 3 passed", 0);
        let fixed = t.observe("Bash", &i4, &r4, P);
        assert_eq!(fixed.len(), 1, "{fixed:?}");
        let f = &fixed[0];
        assert_eq!(f.steps, vec!["Edited src/lib.rs: `foo` -> `let foo = 1; foo`".to_string(), "$ cargo fmt".to_string()]);
        assert_eq!(f.files, vec!["src/lib.rs".to_string()]);
        assert_eq!(f.then, "Then the same command passed.");
        let d = f.draft("Turn 3 request: fix the build");
        assert_eq!((d.kind, d.tag.as_str()), (Kind::Error, TAG));
        assert!(d.ctx.starts_with("Signature: error[E#]: cannot find value `foo` in this scope\n"), "{}", d.ctx);
        assert!(d.text.contains("Cleared by:\n- Edited src/lib.rs"), "{}", d.text);
        assert_eq!(t.open(), 0);

        // Passing again with nothing between: no fix.
        let mut t = Tracker::default();
        t.observe("Bash", &i, &r, P);
        assert!(t.observe("Bash", &i4, &r4, P).is_empty());
    }

    #[test]
    fn the_same_error_again_resets_the_steps_and_another_error_closes_it() {
        let mut t = Tracker::default();
        let (i, r) = bash("make", "fatal: missing header a.h", 2);
        t.observe("Bash", &i, &r, P);
        let edit = json!({"file_path": "/home/u/proj/b.c", "old_string": "x", "new_string": "y"});
        t.observe("Edit", &edit, &json!({}), P);
        // The edit did not help: the same error again.
        assert!(t.observe("Bash", &i, &r, P).is_empty());
        let edit2 = json!({"file_path": "/home/u/proj/a.h", "content": "int a;"});
        t.observe("Write", &edit2, &json!({}), P);
        let (_, r2) = bash("make", "error: undefined reference to `b`", 2);
        let fixed = t.observe("Bash", &i, &r2, P);
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0].steps, vec!["Wrote a.h".to_string()]);
        assert!(fixed[0].then.contains("another error"), "{}", fixed[0].then);
        assert_eq!(t.open(), 1, "the new error is followed");
    }

    #[test]
    fn failures_are_followed_for_a_while_only() {
        let mut t = Tracker::default();
        let (i, r) = bash("npm test", "npm ERR! Test failed", 1);
        t.observe("Bash", &i, &r, P);
        let (i2, r2) = bash("ls", "a", 0);
        for _ in 0..OPEN_FOR + 1 {
            t.observe("Bash", &i2, &r2, P);
        }
        assert_eq!(t.open(), 0);
    }
}
