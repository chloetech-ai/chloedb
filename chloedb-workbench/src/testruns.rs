//! The flaky test radar (SPEC.md, "Fourth build", row 5): every test run an
//! agent makes, read from its Bash calls (a `PostToolUse`, a
//! `PostToolUseFailure`, or, for a call that failed and fired neither, the
//! transcript's `tool_result` at the turn's end), parsed test by test for
//! cargo test (and nextest), pytest, jest, vitest and go test, as rows of
//! `test_runs`; an unknown runner is one row for the whole command with its
//! exit code. From those rows: p50/p99 with `approx_percentile`, pass rates,
//! flaky tests (passed and failed on the same code, or flipping within a day
//! on one commit), newly failing and slow ones.
//!
//! [`after_hook`] is the one place the server calls after each hook: it also
//! hands file edits and transcript paths to [`crate::alerts`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json as AxJson, Router};
use chloedb::query::Params;
use chloedb::Value;
use serde_json::{json, Value as Json};

use crate::db::{get_int, get_ms, get_str, iso, now_ms, opt_int, opt_str, ts};
use crate::server::{blocking, ApiResult, App};
use crate::supervisor::{one_line, ApiError, Supervisor};

/// The most tests one command records.
const MAX_TESTS: usize = 5000;
/// What a failed test keeps of its error.
const ERROR_MAX: usize = 2000;
/// How much of a transcript is read to find a failed call's output.
const TRANSCRIPT_TAIL: u64 = 16 << 20;

pub fn routes() -> Router<App> {
    Router::new().route("/api/tests", get(tests_route)).route("/api/tests/history", get(history_route)).route("/api/tests/runs", get(runs_route))
}

// ------------------------------------------------------------------ parsing

/// One test's result in a run.
#[derive(Clone, Debug, PartialEq)]
pub struct TestResult {
    pub name: String,
    /// `passed`, `failed` or `skipped`.
    pub outcome: &'static str,
    /// Its own time, when the runner reports it.
    pub ms: Option<i64>,
    pub error: Option<String>,
}

/// What a test command's output says.
#[derive(Debug, Default)]
pub struct Parsed {
    /// `cargo`, `nextest`, `pytest`, `jest`, `vitest`, `go`, or `other`.
    pub runner: String,
    pub tests: Vec<TestResult>,
    /// The run's own total, when it prints one.
    pub total_ms: Option<i64>,
}

fn secs_ms(s: &str) -> Option<i64> {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')').trim_end_matches(',');
    if let Some(v) = s.strip_suffix("ms") {
        return v.trim().parse::<f64>().ok().map(|x| x.round() as i64);
    }
    let v = s.strip_suffix('s')?;
    v.trim().parse::<f64>().ok().map(|x| (x * 1000.0).round() as i64)
}

fn clip(lines: &[String]) -> Option<String> {
    let text = lines.iter().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut end = text.len().min(ERROR_MAX);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(text[..end].to_string())
}

/// Adds a result, keeping the first of a name (summaries repeat them) but
/// letting a later failure or time fill in.
fn put(out: &mut Vec<TestResult>, seen: &mut HashMap<String, usize>, r: TestResult) {
    match seen.get(&r.name) {
        Some(&i) => {
            let had = &mut out[i];
            if r.outcome == "failed" {
                had.outcome = "failed";
            }
            if had.ms.is_none() {
                had.ms = r.ms;
            }
            if had.error.is_none() {
                had.error = r.error;
            }
        }
        None => {
            seen.insert(r.name.clone(), out.len());
            out.push(r);
        }
    }
}

/// cargo test's `test a::b ... ok <0.012s>` lines (and its failures'
/// stdout), and nextest's `PASS [ 0.012s] crate a::b`.
fn parse_cargo(text: &str) -> Parsed {
    let mut out = Vec::new();
    let mut seen = HashMap::new();
    let mut total = 0i64;
    let mut any_total = false;
    let mut nextest = false;
    let mut errors: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_err: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if let Some(name) = t.strip_prefix("---- ").and_then(|r| r.strip_suffix(" stdout ----").or_else(|| r.strip_suffix(" stderr ----"))) {
            in_err = Some(name.trim().to_string());
            continue;
        }
        if let Some(name) = &in_err {
            if t.starts_with("---- ") || t == "failures:" || t.starts_with("test result:") || t.starts_with("successes:") {
                in_err = None;
            } else {
                let e = errors.entry(name.clone()).or_default();
                if e.len() < 40 {
                    e.push(line.to_string());
                }
                continue;
            }
        }
        if let Some(rest) = t.strip_prefix("test ") {
            if let Some((name, res)) = rest.split_once(" ... ") {
                let word = res.split_whitespace().next().unwrap_or("");
                let outcome = match word {
                    "ok" => "passed",
                    "FAILED" => "failed",
                    w if w.starts_with("ignored") => "skipped",
                    _ => continue,
                };
                let ms = res.split_once('<').and_then(|(_, r)| r.split_once('>')).and_then(|(s, _)| secs_ms(s));
                put(&mut out, &mut seen, TestResult { name: name.trim().to_string(), outcome, ms, error: None });
                continue;
            }
        }
        if t.starts_with("test result:") {
            if let Some(ms) = t.split("finished in ").nth(1).and_then(secs_ms) {
                total += ms;
                any_total = true;
            }
            continue;
        }
        // nextest: "PASS [   0.012s] crate tests::name"
        let mut w = t.split_whitespace();
        if let Some(status @ ("PASS" | "FAIL" | "SKIP" | "SIGSEGV" | "TIMEOUT" | "LEAK")) = w.next() {
            if let Some(after) = t.split_once('[').and_then(|(_, r)| r.split_once(']')) {
                let (secs, name) = after;
                let name = name.trim();
                if !name.is_empty() {
                    nextest = true;
                    let outcome = match status {
                        "PASS" | "LEAK" => "passed",
                        "SKIP" => "skipped",
                        _ => "failed",
                    };
                    put(&mut out, &mut seen, TestResult { name: name.to_string(), outcome, ms: secs_ms(secs), error: None });
                }
            }
        }
    }
    for r in out.iter_mut().filter(|r| r.outcome == "failed") {
        r.error = errors.get(&r.name).and_then(|e| clip(e));
    }
    Parsed { runner: if nextest { "nextest" } else { "cargo" }.into(), tests: out, total_ms: any_total.then_some(total) }
}

const PY_STATUS: [&str; 6] = ["PASSED", "FAILED", "ERROR", "SKIPPED", "XFAIL", "XPASS"];

fn py_outcome(s: &str) -> &'static str {
    match s {
        "PASSED" | "XPASS" => "passed",
        "FAILED" | "ERROR" => "failed",
        _ => "skipped",
    }
}

/// pytest's `-v` lines (`tests/a.py::t PASSED [ 10%]`), its short summary
/// (`FAILED tests/a.py::t - Assert…`), `--durations` lines and failure
/// sections.
fn parse_pytest(text: &str) -> Parsed {
    let mut out = Vec::new();
    let mut seen = HashMap::new();
    let mut durations: HashMap<String, i64> = HashMap::new();
    let mut total = None;
    let mut sections: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_sec: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim();
        // A failure's section: "____ test_name ____" (or "____ Class.test ____").
        if t.starts_with("___") && t.ends_with("___") && t.len() > 6 {
            let name = t.trim_matches('_').trim().to_string();
            in_sec = (!name.is_empty() && !name.contains(' ')).then_some(name);
            continue;
        }
        if t.starts_with("===") {
            in_sec = None;
            if t.contains(" in ") && (t.contains("passed") || t.contains("failed") || t.contains("error")) {
                total = t.rsplit(" in ").next().and_then(|r| r.split_whitespace().next()).and_then(secs_ms);
            }
            continue;
        }
        if let Some(name) = &in_sec {
            let e = sections.entry(name.clone()).or_default();
            if e.len() < 60 {
                e.push(line.to_string());
            }
            continue;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        // --durations: "0.52s call     tests/a.py::t"
        if words.len() >= 3 && words[1] == "call" && words[2].contains("::") {
            if let Some(ms) = secs_ms(words[0]) {
                durations.insert(words[2].to_string(), ms);
            }
            continue;
        }
        for (i, w) in words.iter().enumerate() {
            if !PY_STATUS.contains(w) {
                continue;
            }
            // "STATUS nodeid" (summary, xdist) or "nodeid STATUS" (-v).
            let node = words.get(i + 1).filter(|n| n.contains("::")).or_else(|| i.checked_sub(1).and_then(|j| words.get(j)).filter(|n| n.contains("::")));
            if let Some(node) = node {
                put(&mut out, &mut seen, TestResult { name: node.to_string(), outcome: py_outcome(w), ms: None, error: None });
                break;
            }
        }
    }
    for r in out.iter_mut() {
        r.ms = durations.get(&r.name).copied();
        if r.outcome == "failed" {
            let short = r.name.rsplit("::").next().unwrap_or("");
            let class_form = r.name.split("::").skip(1).collect::<Vec<_>>().join(".");
            let sec = sections.get(short).or_else(|| sections.get(&class_form));
            if let Some(lines) = sec {
                // The lines that say what went wrong ("E   assert …"), else the section's end.
                let es: Vec<String> = lines.iter().filter(|l| l.trim_start().starts_with("E ")).cloned().collect();
                r.error = clip(if es.is_empty() { &lines[lines.len().saturating_sub(12)..] } else { &es });
            }
        }
    }
    Parsed { runner: "pytest".into(), tests: out, total_ms: total }
}

fn marker(s: &str) -> Option<(&'static str, &str)> {
    for (m, o) in [("✓", "passed"), ("✔", "passed"), ("√", "passed"), ("✕", "failed"), ("✗", "failed"), ("×", "failed"), ("✖", "failed"), ("○", "skipped"), ("↓", "skipped")] {
        if let Some(rest) = s.strip_prefix(m) {
            return Some((o, rest.trim_start()));
        }
    }
    None
}

/// A trailing time: "(5 ms)", "5ms", "(1.2 s)".
fn trailing_time(s: &str) -> (String, Option<i64>) {
    let s = s.trim_end();
    if s.ends_with(')') {
        if let Some(open) = s.rfind('(') {
            if let Some(ms) = secs_ms(&s[open..].replace(' ', "")) {
                return (s[..open].trim_end().to_string(), Some(ms));
            }
        }
    }
    if let Some((head, last)) = s.rsplit_once(' ') {
        if last.ends_with("ms") || (last.ends_with('s') && last[..last.len() - 1].parse::<f64>().is_ok()) {
            if let Some(ms) = secs_ms(last) {
                return (head.trim_end().to_string(), Some(ms));
            }
        }
    }
    (s.to_string(), None)
}

/// jest (`PASS file`, indented describes, `✓ name (5 ms)`, `● a › b`
/// failures) and vitest (`✓ file > suite > name 5ms`, `FAIL file > …`).
fn parse_js(text: &str) -> Parsed {
    let mut out = Vec::new();
    let mut seen = HashMap::new();
    let mut file: Option<String> = None;
    let mut describes: Vec<(usize, String)> = Vec::new();
    let mut vitest = false;
    let mut errors: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_err: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        let indent = line.len() - t.len();
        if let Some(rest) = t.strip_prefix("● ") {
            let name = rest.trim().to_string();
            in_err = (!name.contains("Test suite failed")).then_some(name);
            continue;
        }
        if let Some(name) = &in_err {
            if t.starts_with("PASS ") || t.starts_with("FAIL ") || t.starts_with("Test Suites:") {
                in_err = None;
            } else {
                let e = errors.entry(name.clone()).or_default();
                if e.len() < 30 && !t.is_empty() {
                    e.push(t.to_string());
                }
                continue;
            }
        }
        for (head, failed) in [("PASS ", false), ("FAIL ", true)] {
            if let Some(rest) = t.strip_prefix(head) {
                let rest = rest.trim();
                if rest.contains(" > ") {
                    // vitest's failure summary: "FAIL  file > suite > name"
                    vitest = true;
                    if failed {
                        let name = rest.split(" > ").map(str::trim).collect::<Vec<_>>().join(" › ");
                        let (name, _) = trailing_time(&name);
                        put(&mut out, &mut seen, TestResult { name, outcome: "failed", ms: None, error: None });
                    }
                } else {
                    file = Some(rest.split_whitespace().next().unwrap_or("").to_string());
                    describes.clear();
                }
            }
        }
        let Some((outcome, rest)) = marker(t) else {
            // A describe block's title: an indented line under a file, no marker.
            if file.is_some() && indent >= 2 && !t.is_empty() && !t.starts_with("at ") && !t.contains(':') && t.len() < 200 {
                while describes.last().is_some_and(|(i, _)| *i >= indent) {
                    describes.pop();
                }
                describes.push((indent, t.to_string()));
            }
            continue;
        };
        let rest = rest.strip_prefix("skipped ").or_else(|| rest.strip_prefix("todo ")).unwrap_or(rest);
        let (name, ms) = trailing_time(rest);
        if name.contains(" > ") || name.contains(".test.") || name.contains(".spec.") {
            // vitest: "file > suite > name"; a file's own line "file (3 tests)" is not a test.
            if name.ends_with("tests)") || name.ends_with("test)") || !name.contains(" > ") {
                continue;
            }
            vitest = true;
            let name = name.split(" > ").map(str::trim).collect::<Vec<_>>().join(" › ");
            put(&mut out, &mut seen, TestResult { name, outcome, ms, error: None });
            continue;
        }
        while describes.last().is_some_and(|(i, _)| *i >= indent) {
            describes.pop();
        }
        let mut parts: Vec<String> = file.iter().cloned().collect();
        parts.extend(describes.iter().map(|(_, d)| d.clone()));
        parts.push(name);
        put(&mut out, &mut seen, TestResult { name: parts.join(" › "), outcome, ms, error: None });
    }
    for r in out.iter_mut().filter(|r| r.outcome == "failed") {
        // jest names a failure "describe › test" without its file.
        let key = errors.keys().find(|k| r.name.ends_with(k.as_str())).cloned();
        r.error = key.and_then(|k| clip(&errors[&k]));
    }
    Parsed { runner: if vitest { "vitest" } else { "jest" }.into(), tests: out, total_ms: None }
}

/// go test -v: `--- PASS: TestX (0.00s)`, the package from `ok  pkg 0.1s`
/// or `FAIL pkg 0.2s`, and a failure's `x_test.go:12:` lines.
fn parse_go(text: &str) -> Parsed {
    let mut out: Vec<TestResult> = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    let mut logs: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<String> = None;
    let mut total = 0i64;
    let mut any = false;
    for raw in text.lines() {
        let t = raw.trim();
        if let Some(name) = t.strip_prefix("=== RUN") {
            current = Some(name.trim().to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix("--- ") {
            let (status, rest) = rest.split_once(':').unwrap_or(("", ""));
            let outcome = match status {
                "PASS" => "passed",
                "FAIL" => "failed",
                "SKIP" => "skipped",
                _ => continue,
            };
            let (name, ms) = trailing_time(rest.trim());
            let error = if outcome == "failed" { logs.get(&name).and_then(|l| clip(l)) } else { None };
            pending.push(out.len());
            out.push(TestResult { name, outcome, ms, error });
            continue;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        if matches!(words.first(), Some(&"ok") | Some(&"FAIL")) && words.len() >= 2 && !words[1].starts_with('-') {
            let pkg = words[1];
            if let Some(ms) = words.get(2).and_then(|w| secs_ms(w)) {
                total += ms;
                any = true;
            }
            for i in pending.drain(..) {
                out[i].name = format!("{pkg} › {}", out[i].name);
            }
            continue;
        }
        if let Some(c) = &current {
            if raw.starts_with("    ") && t.contains("_test.go:") {
                let l = logs.entry(c.split('/').next().unwrap_or(c).to_string()).or_default();
                if l.len() < 20 {
                    l.push(t.to_string());
                }
                logs.entry(c.clone()).or_default().push(t.to_string());
            }
        }
    }
    Parsed { runner: "go".into(), tests: out, total_ms: any.then_some(total) }
}

/// Parses a test command's output with the parser its command names first,
/// then the others; `runner: "other"` and no tests when none finds any.
pub fn parse(command: &str, output: &str) -> Parsed {
    let c = command.to_ascii_lowercase();
    let mut order: Vec<fn(&str) -> Parsed> = Vec::new();
    if c.contains("cargo") {
        order.push(parse_cargo);
    }
    if c.contains("pytest") || c.contains("tox") {
        order.push(parse_pytest);
    }
    if c.contains("go test") {
        order.push(parse_go);
    }
    if c.contains("jest") || c.contains("vitest") || c.contains("npm") || c.contains("yarn") || c.contains("pnpm") || c.contains("bun") || c.contains("npx") {
        order.push(parse_js);
    }
    for f in [parse_cargo as fn(&str) -> Parsed, parse_pytest, parse_go, parse_js] {
        if !order.iter().any(|g| *g as usize == f as usize) {
            order.push(f);
        }
    }
    for f in order {
        let mut p = f(output);
        if !p.tests.is_empty() {
            p.tests.truncate(MAX_TESTS);
            return p;
        }
    }
    Parsed { runner: "other".into(), tests: Vec::new(), total_ms: None }
}

// ---------------------------------------------------------------- capture

/// The one call the server makes after each hook was recorded: test runs
/// from Bash calls, file edits and transcript paths for the alerts. Never
/// fails and never waits on git or a transcript: those run on a thread.
pub fn after_hook(sup: &Arc<Supervisor>, id: &str, hook: &Json) {
    let s = |k: &str| hook.get(k).and_then(Json::as_str).unwrap_or("");
    let kind = s("hook_event_name");
    let tool = s("tool_name");
    if !s("transcript_path").is_empty() {
        crate::alerts::note_transcript(id, s("transcript_path"));
    }
    match (kind, tool) {
        ("PostToolUse" | "PostToolUseFailure", "Bash") => {
            let cmd = hook["tool_input"]["command"].as_str().unwrap_or("");
            if crate::flow::is_test(cmd) {
                let (sup, id, hook) = (sup.clone(), id.to_string(), hook.clone());
                let _ = std::thread::Builder::new().name(format!("tests-{id}")).spawn(move || record_from_hook(&sup, &id, &hook));
            }
        }
        ("PostToolUse", "Edit" | "Write" | "MultiEdit" | "NotebookEdit") => crate::alerts::record_edit(sup, id, hook),
        ("Stop", _) => {
            let (sup, id, path) = (sup.clone(), id.to_string(), s("transcript_path").to_string());
            let _ = std::thread::Builder::new().name(format!("tests-stop-{id}")).spawn(move || recover_turn(&sup, &id, &path));
        }
        _ => {}
    }
}

/// The text of a tool's response or a transcript's `tool_result` content.
fn text_of(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Array(a) => a.iter().map(|c| c.get("text").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| text_of(c))).collect::<Vec<_>>().join("\n"),
        Json::Object(o) => {
            let mut parts = Vec::new();
            for k in ["stdout", "stderr", "output", "error", "content"] {
                if let Some(x) = o.get(k) {
                    let t = text_of(x);
                    if !t.is_empty() {
                        parts.push(t);
                    }
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

/// "Exit code 101" in a failure's text, else 1.
fn exit_code_in(text: &str) -> i64 {
    text.lines().find_map(|l| l.trim().strip_prefix("Exit code ").and_then(|n| n.trim().parse().ok())).unwrap_or(1)
}

fn hash8(s: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

/// One Bash call that ran tests, as found.
struct Call {
    command: String,
    output: String,
    exit_code: i64,
    use_id: Option<String>,
    end_ms: i64,
}

fn record_from_hook(sup: &Supervisor, id: &str, hook: &Json) {
    let r = &hook["tool_response"];
    if r.get("interrupted").and_then(Json::as_bool) == Some(true) {
        return;
    }
    let failed = hook["hook_event_name"] == "PostToolUseFailure" || r.get("is_error").and_then(Json::as_bool) == Some(true);
    let mut output = text_of(r);
    if let Some(e) = hook.get("error").and_then(Json::as_str) {
        output = format!("{output}\n{e}");
    }
    let call = Call {
        command: hook["tool_input"]["command"].as_str().unwrap_or("").to_string(),
        exit_code: if failed { exit_code_in(&output) } else { 0 },
        output,
        use_id: hook["tool_use_id"].as_str().map(str::to_string),
        end_ms: now_ms(),
    };
    record(sup, id, call);
}

/// The agent's recent events, newest first: (seq, ts_ms, kind, data).
fn recent_events(sup: &Supervisor, agent: &str, n: usize) -> Vec<(i64, i64, String, Json)> {
    let p = Params::new().set("agent", Value::Str(agent.to_string()));
    let rows = sup.db().query(&format!("SELECT seq, ts, kind, data FROM events WHERE agent = :agent ORDER BY seq DESC LIMIT {n}"), &p).unwrap_or_default();
    rows.iter()
        .map(|r| (get_int(&r[0]).unwrap_or(0), get_ms(&r[1]).unwrap_or(0), get_str(&r[2]).unwrap_or_default(), get_str(&r[3]).and_then(|d| serde_json::from_str(&d).ok()).unwrap_or(Json::Null)))
        .collect()
}

/// The `PreToolUse` of a call: by `tool_use_id`, else the newest of its
/// command; (seq, ts_ms).
fn pre_of(evs: &[(i64, i64, String, Json)], use_id: Option<&str>, command: &str) -> Option<(i64, i64)> {
    let pres = || evs.iter().filter(|e| e.2 == "PreToolUse" && e.3["tool_name"] == "Bash");
    use_id
        .and_then(|u| pres().find(|e| e.3["tool_use_id"].as_str() == Some(u)))
        .or_else(|| pres().find(|e| e.3["tool_input"]["command"].as_str() == Some(command)))
        .map(|e| (e.0, e.1))
}

/// The commit of a checkout and a hash of its uncommitted changes
/// (`clean` without any): together, "the same code".
fn code_of(dir: &Path) -> (Option<String>, String) {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let commit = git(&["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let diff = git(&["diff", "HEAD", "--no-ext-diff", "--no-color"]).unwrap_or_default();
    let untracked = git(&["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    let tree = if diff.trim().is_empty() && untracked.trim().is_empty() { "clean".to_string() } else { hash8(&format!("{diff}\u{0}{untracked}")) };
    (commit, tree)
}

/// Writes a call's tests as rows of `test_runs` (one row for the command
/// when its output names none). A call already recorded (its run id is
/// made from its `tool_use_id`) is replaced.
fn record(sup: &Supervisor, agent: &str, call: Call) {
    let Some(a) = sup.agent_row(agent) else { return };
    let evs = recent_events(sup, agent, 400);
    let pre = pre_of(&evs, call.use_id.as_deref(), &call.command);
    let seq = pre.map(|p| p.0).or_else(|| evs.first().map(|e| e.0));
    let start = pre.map(|p| p.1);
    let run = match &call.use_id {
        Some(u) => format!("t{}", hash8(&format!("{agent}:{u}"))),
        None => crate::levels::new_id("t"),
    };
    let parsed = parse(&call.command, &call.output);
    let run_ms = start.map(|s| (call.end_ms - s).max(0)).or(parsed.total_ms);
    let (commit, tree) = code_of(Path::new(&a.cwd));
    let mut tests = parsed.tests;
    let runner = if tests.is_empty() { "other".to_string() } else { parsed.runner };
    if tests.is_empty() {
        let outcome = if call.exit_code == 0 { "passed" } else { "failed" };
        let error = (outcome == "failed").then(|| {
            let lines: Vec<String> = call.output.lines().rev().take(15).map(str::to_string).collect::<Vec<_>>().into_iter().rev().collect();
            clip(&lines)
        });
        tests.push(TestResult { name: one_line(&call.command, 160), outcome, ms: None, error: error.flatten() });
    }
    let now = now_ms();
    let _ = sup.db().query("DELETE FROM test_runs WHERE run = :run", &Params::new().set("run", Value::Str(run.clone())));
    for (i, t) in tests.iter().enumerate() {
        let own = t.ms.is_some();
        let p = Params::new()
            .set("id", Value::Str(format!("{run}:{i:05}")))
            .set("run", Value::Str(run.clone()))
            .set("repo", Value::Str(a.repo.clone()))
            .set("agent", Value::Str(agent.to_string()))
            .set("commit", opt_str(&commit))
            .set("tree", Value::Str(tree.clone()))
            .set("runner", Value::Str(runner.clone()))
            .set("command", Value::Str(one_line(&call.command, 2000)))
            .set("test", Value::Str(t.name.clone()))
            .set("outcome", Value::Str(t.outcome.to_string()))
            .set("passed", Value::Bool(t.outcome == "passed"))
            .set("ms", opt_int(t.ms.or(run_ms)))
            .set("own_ms", Value::Bool(own))
            .set("run_ms", opt_int(run_ms))
            .set("exit_code", Value::Int(call.exit_code))
            .set("error", opt_str(&t.error))
            .set("use_id", opt_str(&call.use_id))
            .set("ts", ts(call.end_ms.min(now)))
            .set("seq", opt_int(seq));
        let _ = sup.db().query(
            "INSERT OR REPLACE INTO test_runs (id, run, repo, agent, \"commit\", tree, runner, command, test, outcome, passed, ms, own_ms, run_ms, exit_code, error, use_id, ts, seq) VALUES (:id, :run, :repo, :agent, :commit, :tree, :runner, :command, :test, :outcome, :passed, :ms, :own_ms, :run_ms, :exit_code, :error, :use_id, :ts, :seq)",
            &p,
        );
    }
}

/// At a turn's end: its Bash test calls that fired no `PostToolUse` (they
/// failed, and Claude Code fires none then) are read from the transcript.
fn recover_turn(sup: &Supervisor, agent: &str, transcript: &str) {
    let evs = recent_events(sup, agent, 600);
    let mut turn = Vec::new();
    for e in &evs {
        if e.2 == "UserPromptSubmit" {
            break;
        }
        turn.push(e);
    }
    let done: HashSet<&str> = turn.iter().filter(|e| e.2.starts_with("PostToolUse")).filter_map(|e| e.3["tool_use_id"].as_str()).collect();
    let open: Vec<(String, String)> = turn
        .iter()
        .filter(|e| e.2 == "PreToolUse" && e.3["tool_name"] == "Bash" && crate::flow::is_test(e.3["tool_input"]["command"].as_str().unwrap_or("")))
        .filter_map(|e| {
            let u = e.3["tool_use_id"].as_str()?;
            (!done.contains(u)).then(|| (u.to_string(), e.3["tool_input"]["command"].as_str().unwrap_or("").to_string()))
        })
        .collect();
    if open.is_empty() || transcript.is_empty() {
        return;
    }
    let results = tool_results(Path::new(transcript), &open.iter().map(|o| o.0.as_str()).collect::<Vec<_>>());
    for (use_id, command) in open {
        let Some((text, is_error, at)) = results.get(&use_id) else { continue };
        let exists = sup.db().query("SELECT id FROM test_runs WHERE use_id = :u LIMIT 1", &Params::new().set("u", Value::Str(use_id.clone()))).map(|r| !r.is_empty()).unwrap_or(false);
        if exists {
            continue;
        }
        let exit_code = if *is_error { exit_code_in(text) } else { 0 };
        record(sup, agent, Call { command, output: text.clone(), exit_code, use_id: Some(use_id), end_ms: at.unwrap_or_else(now_ms) });
    }
}

/// The `tool_result`s of some tool calls in a transcript's tail: their
/// text, whether they are errors, and when (ms).
fn tool_results(path: &Path, ids: &[&str]) -> HashMap<String, (String, bool, Option<i64>)> {
    let mut out = HashMap::new();
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return out;
    }
    let Ok(mut f) = std::fs::File::open(path) else { return out };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > TRANSCRIPT_TAIL && f.seek(SeekFrom::Start(len - TRANSCRIPT_TAIL)).is_err() {
        return out;
    }
    let mut buf = Vec::new();
    if f.take(TRANSCRIPT_TAIL).read_to_end(&mut buf).is_err() {
        return out;
    }
    for line in String::from_utf8_lossy(&buf).lines() {
        if !line.contains("tool_result") || !ids.iter().any(|id| line.contains(id)) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Json>(line) else { continue };
        let at = v["timestamp"].as_str().and_then(chloedb::timestamp::parse).map(|t| t / 1000);
        for c in v["message"]["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "tool_result") {
            if let Some(id) = c["tool_use_id"].as_str().filter(|id| ids.contains(id)) {
                out.insert(id.to_string(), (text_of(&c["content"]), c["is_error"] == true, at));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ stats

/// A row of `test_runs` as the stats read it.
#[derive(Clone, Debug)]
struct Row {
    test: String,
    run: String,
    agent: String,
    commit: Option<String>,
    tree: String,
    outcome: String,
    ms: Option<i64>,
    own_ms: bool,
    ts: i64,
    runner: String,
    command: String,
    exit_code: Option<i64>,
    seq: Option<i64>,
    error: Option<String>,
}

const ROW_COLS: &str = "test, run, agent, \"commit\", tree, outcome, ms, own_ms, ts, runner, command, exit_code, seq, error";

fn row_of(r: &[Value]) -> Row {
    Row {
        test: get_str(&r[0]).unwrap_or_default(),
        run: get_str(&r[1]).unwrap_or_default(),
        agent: get_str(&r[2]).unwrap_or_default(),
        commit: get_str(&r[3]),
        tree: get_str(&r[4]).unwrap_or_default(),
        outcome: get_str(&r[5]).unwrap_or_default(),
        ms: get_int(&r[6]),
        own_ms: get_int(&r[7]).unwrap_or(0) != 0,
        ts: get_ms(&r[8]).unwrap_or(0),
        runner: get_str(&r[9]).unwrap_or_default(),
        command: get_str(&r[10]).unwrap_or_default(),
        exit_code: get_int(&r[11]),
        seq: get_int(&r[12]),
        error: get_str(&r[13]),
    }
}

fn rows_where(sup: &Supervisor, cond: &str, p: &Params, limit: usize) -> Vec<Row> {
    sup.db().query(&format!("SELECT {ROW_COLS} FROM test_runs WHERE {cond} ORDER BY ts ASC LIMIT {limit}"), p).unwrap_or_default().iter().map(|r| row_of(r)).collect()
}

/// A test's verdicts over its runs (oldest first).
#[derive(Debug, Default, Clone)]
pub struct Verdict {
    pub flaky: bool,
    pub flaky_why: Option<String>,
    pub newly_failing: bool,
}

fn short(sha: &Option<String>) -> String {
    sha.as_deref().map(|s| s.chars().take(8).collect()).unwrap_or_else(|| "no commit".into())
}

/// Flaky: passed and failed on the same code (commit and uncommitted
/// changes), or flipping twice or more within a day on one commit. Newly
/// failing: its last run failed after a pass.
fn verdict(runs: &[&Row]) -> Verdict {
    let mut v = Verdict::default();
    let decided: Vec<&&Row> = runs.iter().filter(|r| r.outcome == "passed" || r.outcome == "failed").collect();
    let mut by_code: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    for r in &decided {
        let e = by_code.entry((r.commit.clone().unwrap_or_default(), r.tree.clone())).or_default();
        if r.outcome == "passed" {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
    }
    if let Some(((commit, _), (p, f))) = by_code.iter().find(|(_, (p, f))| *p > 0 && *f > 0) {
        v.flaky = true;
        v.flaky_why = Some(format!("passed and failed on {} with the same code ({f} of {} runs failed)", short(&Some(commit.clone()).filter(|c| !c.is_empty())), p + f));
    } else {
        // Flips on one commit within a day.
        let mut by_commit: BTreeMap<String, Vec<&&Row>> = BTreeMap::new();
        for r in &decided {
            by_commit.entry(r.commit.clone().unwrap_or_default()).or_default().push(r);
        }
        for (commit, rs) in by_commit {
            for (i, first) in rs.iter().enumerate() {
                let day: Vec<&&&Row> = rs[i..].iter().take_while(|r| r.ts - first.ts <= 86_400_000).collect();
                let flips = day.windows(2).filter(|w| w[0].outcome != w[1].outcome).count();
                if flips >= 2 {
                    v.flaky = true;
                    v.flaky_why = Some(format!("flipped {flips} times within a day on {}, no commit between", short(&Some(commit.clone()).filter(|c| !c.is_empty()))));
                    break;
                }
            }
            if v.flaky {
                break;
            }
        }
    }
    if let Some(last) = decided.last() {
        v.newly_failing = last.outcome == "failed" && decided.iter().any(|r| r.outcome == "passed");
    }
    v
}

/// The nearest-rank percentile of sorted values.
fn percentile(sorted: &[i64], p: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    Some(sorted[rank - 1])
}

fn is_slow(p50: Option<i64>, p99: Option<i64>) -> bool {
    match (p50, p99) {
        (_, Some(p99)) if p99 >= 5000 => true,
        (Some(p50), Some(p99)) => p99 >= 1000 && p99 >= 3 * p50.max(1),
        _ => false,
    }
}

/// p50 and p99 per test with ChloeDB's `approx_percentile`; `None` when
/// the statement does not run (then they are computed here).
fn sql_percentiles(sup: &Supervisor, repo: &str, since: i64) -> Option<HashMap<String, (Option<i64>, Option<i64>)>> {
    let p = Params::new().set("repo", Value::Str(repo.to_string())).set("since", ts(since));
    let rows = sup
        .db()
        .query("SELECT test, approx_percentile(ms, 0.5) AS p50, approx_percentile(ms, 0.99) AS p99 FROM test_runs WHERE repo = :repo AND ts >= :since AND ms IS NOT NULL GROUP BY test LIMIT 100000", &p)
        .ok()?;
    let f = |v: &Value| match v {
        Value::Float(x) => Some(x.round() as i64),
        other => get_int(other),
    };
    Some(rows.iter().filter_map(|r| Some((get_str(&r[0])?, (f(&r[1]), f(&r[2]))))).collect())
}

/// One test's numbers and verdicts, as `/api/tests` answers them.
fn test_json(test: &str, runs: &[&Row], pct: Option<(Option<i64>, Option<i64>)>) -> (Json, Verdict) {
    let passed = runs.iter().filter(|r| r.outcome == "passed").count();
    let failed = runs.iter().filter(|r| r.outcome == "failed").count();
    let decided = (passed + failed).max(1);
    let (p50, p99) = pct.unwrap_or_else(|| {
        let mut ms: Vec<i64> = runs.iter().filter_map(|r| r.ms).collect();
        ms.sort_unstable();
        (percentile(&ms, 0.5), percentile(&ms, 0.99))
    });
    let v = verdict(runs);
    let last = runs.last().copied();
    let mut agents: Vec<&str> = Vec::new();
    for r in runs {
        if !agents.contains(&r.agent.as_str()) {
            agents.push(&r.agent);
        }
    }
    let history: Vec<Json> = runs[runs.len().saturating_sub(20)..].iter().map(|r| json!({ "outcome": r.outcome, "ts_ms": r.ts, "agent": r.agent, "commit": r.commit, "ms": r.ms })).collect();
    let j = json!({
        "test": test, "runner": last.map(|r| r.runner.clone()), "runs": runs.len(), "passed": passed, "failed": failed,
        "pass_rate": passed as f64 / decided as f64, "p50_ms": p50, "p99_ms": p99, "own_ms": runs.iter().any(|r| r.own_ms),
        "flaky": v.flaky, "flaky_why": v.flaky_why, "slow": is_slow(p50, p99), "newly_failing": v.newly_failing,
        "last": last.map(|r| json!({ "outcome": r.outcome, "ts_ms": r.ts, "ts": iso(r.ts), "agent": r.agent, "commit": r.commit })),
        "agents": agents, "history": history,
    });
    (j, v)
}

/// The commands of some rows, newest first: a summary per run.
fn commands_of(rows: &[Row], newly: &HashSet<String>, flaky: &HashSet<String>) -> Vec<Json> {
    let mut runs: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        runs.entry(r.run.clone()).or_default().push(r);
    }
    let mut out: Vec<(i64, Json)> = runs
        .into_iter()
        .map(|(run, rs)| {
            let first = rs[0];
            let n = |o: &str| rs.iter().filter(|r| r.outcome == o).count();
            let known = first.runner != "other";
            let failed_tests: Vec<&str> = rs.iter().filter(|r| r.outcome == "failed").map(|r| r.test.as_str()).collect();
            let run_ms = rs.iter().filter(|r| !r.own_ms).find_map(|r| r.ms).or_else(|| known.then(|| rs.iter().filter_map(|r| r.ms).sum()));
            let seqs: Vec<i64> = first.seq.into_iter().collect();
            (
                first.ts,
                json!({
                    "run": run, "agent": first.agent, "command": first.command, "runner": first.runner, "ts_ms": first.ts, "ts": iso(first.ts),
                    "exit_code": first.exit_code, "tests": if known { rs.len() } else { 0 }, "passed": if known { n("passed") } else { 0 },
                    "failed": if known { n("failed") } else { 0 }, "ignored": if known { n("skipped") } else { 0 }, "ms": run_ms, "commit": first.commit,
                    "seq": first.seq, "seqs": seqs, "failed_tests": failed_tests, "running": false,
                    "newly_failing": failed_tests.iter().filter(|t| newly.contains(**t)).collect::<Vec<_>>(),
                    "flaky_tests": rs.iter().map(|r| r.test.as_str()).filter(|t| flaky.contains(*t)).collect::<Vec<_>>(),
                }),
            )
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, j)| j).collect()
}

/// `GET /api/tests?repo=&days=7`: the radar of a repository.
pub fn radar(sup: &Supervisor, repo: Option<&str>, days: i64) -> Result<Json, ApiError> {
    let repo = repo_of(sup, repo)?;
    let days = days.clamp(1, 90);
    let now = now_ms();
    let since = now - days * 86_400_000;
    let p = Params::new().set("repo", Value::Str(repo.clone())).set("since", ts(since));
    let rows = rows_where(sup, "repo = :repo AND ts >= :since", &p, 200_000);
    let pcts = sql_percentiles(sup, &repo, since);
    let mut by_test: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        by_test.entry(r.test.as_str()).or_default().push(r);
    }
    let mut tests = Vec::new();
    let (mut newly, mut flaky) = (HashSet::new(), HashSet::new());
    for (test, runs) in &by_test {
        let (j, v) = test_json(test, runs, pcts.as_ref().and_then(|m| m.get(*test).copied()));
        if v.newly_failing {
            newly.insert(test.to_string());
        }
        if v.flaky {
            flaky.insert(test.to_string());
        }
        tests.push(j);
    }
    let commands = commands_of(&rows, &newly, &flaky);
    let count = |k: &str| tests.iter().filter(|t| t[k] == true).count();
    let passed = rows.iter().filter(|r| r.outcome == "passed").count();
    let failed = rows.iter().filter(|r| r.outcome == "failed").count();
    let mut agent_ids: Vec<&str> = rows.iter().map(|r| r.agent.as_str()).collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let agents: Vec<Json> = agent_ids.iter().filter_map(|id| sup.agent_json_of(id)).collect();
    let mut missing = Vec::new();
    if pcts.is_none() && !rows.is_empty() {
        missing.push("approx_percentile did not run here, so p50 and p99 were computed from the rows".to_string());
    }
    Ok(json!({
        "repo": repo,
        "window": { "from_ms": since, "to_ms": now, "days": days },
        "totals": { "tests": tests.len(), "runs": rows.len(), "commands": commands.len(), "passed": passed, "failed": failed,
                    "pass_rate": if passed + failed > 0 { passed as f64 / (passed + failed) as f64 } else { 1.0 },
                    "flaky": count("flaky"), "slow": count("slow"), "newly_failing": count("newly_failing") },
        "tests": tests,
        "commands": commands.into_iter().take(50).collect::<Vec<_>>(),
        "agents": agents,
        "percentiles": if pcts.is_some() { "approx_percentile" } else { "rust" },
        "missing": missing,
    }))
}

/// `GET /api/tests/history?repo=&test=`: a test's runs, newest first.
pub fn history(sup: &Supervisor, repo: Option<&str>, test: &str) -> Result<Json, ApiError> {
    let repo = repo_of(sup, repo)?;
    let p = Params::new().set("repo", Value::Str(repo.clone())).set("test", Value::Str(test.to_string()));
    let rows = rows_where(sup, "repo = :repo AND test = :test", &p, 100_000);
    if rows.is_empty() {
        return Err(ApiError::not_found(format!("no runs of {test} in {repo}")));
    }
    let refs: Vec<&Row> = rows.iter().collect();
    let v = verdict(&refs);
    let mut agent_ids: Vec<&str> = rows.iter().map(|r| r.agent.as_str()).collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let runs: Vec<Json> = rows
        .iter()
        .rev()
        .take(500)
        .map(|r| json!({ "run": r.run, "agent": r.agent, "outcome": r.outcome, "ms": r.ms, "own_ms": r.own_ms, "ts_ms": r.ts, "ts": iso(r.ts), "commit": r.commit, "tree": r.tree, "command": r.command, "exit_code": r.exit_code, "seq": r.seq, "error": r.error }))
        .collect();
    Ok(json!({
        "test": test, "repo": repo, "runner": rows.last().map(|r| r.runner.clone()), "runs": runs,
        "flaky": v.flaky, "flaky_why": v.flaky_why, "newly_failing": v.newly_failing,
        "agents": agent_ids.iter().filter_map(|id| sup.agent_json_of(id)).collect::<Vec<_>>(),
    }))
}

/// `GET /api/tests/runs?agent=`: an agent's test commands, newest first,
/// with what failed, what is flaky and what newly fails (Story's chip).
pub fn agent_runs(sup: &Supervisor, agent: &str) -> Result<Json, ApiError> {
    let a = sup.agent_row(agent).ok_or_else(|| ApiError::not_found(format!("no agent {agent}")))?;
    let p = Params::new().set("agent", Value::Str(agent.to_string()));
    let rows = rows_where(sup, "agent = :agent", &p, 100_000);
    // Verdicts over the repository's last week, for the tests this agent ran.
    let names: HashSet<&str> = rows.iter().map(|r| r.test.as_str()).collect();
    let since = now_ms() - 7 * 86_400_000;
    let rp = Params::new().set("repo", Value::Str(a.repo.clone())).set("since", ts(since));
    let all = rows_where(sup, "repo = :repo AND ts >= :since", &rp, 200_000);
    let mut by_test: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in all.iter().filter(|r| names.contains(r.test.as_str())) {
        by_test.entry(r.test.as_str()).or_default().push(r);
    }
    let (mut newly, mut flaky) = (HashSet::new(), HashSet::new());
    for (t, runs) in by_test {
        let v = verdict(&runs);
        if v.newly_failing {
            newly.insert(t.to_string());
        }
        if v.flaky {
            flaky.insert(t.to_string());
        }
    }
    Ok(json!({ "runs": commands_of(&rows, &newly, &flaky) }))
}

/// Tests that failed in a window, for an incident's board: per test of
/// the agent (or every agent), its last 20 runs.
pub fn board_tests(sup: &Supervisor, repo: &str, tests: &[String]) -> Vec<Json> {
    let mut out = Vec::new();
    for t in tests.iter().take(6) {
        let p = Params::new().set("repo", Value::Str(repo.to_string())).set("test", Value::Str(t.clone()));
        let rows = rows_where(sup, "repo = :repo AND test = :test", &p, 10_000);
        if rows.is_empty() {
            continue;
        }
        let refs: Vec<&Row> = rows.iter().collect();
        let (mut j, _) = test_json(t, &refs, None);
        j["error"] = json!(rows.iter().rev().find_map(|r| r.error.clone()));
        out.push(j);
    }
    out
}

fn fmt_ms(ms: Option<i64>) -> String {
    match ms {
        None => "?".into(),
        Some(m) if m < 1000 => format!("{m} ms"),
        Some(m) if m < 60_000 => format!("{:.1} s", m as f64 / 1000.0),
        Some(m) => format!("{:.1} min", m as f64 / 60_000.0),
    }
}

/// `test_history`'s text for the radar: the flagged tests first.
pub fn radar_text(v: &Json) -> String {
    let t = &v["totals"];
    let mut out = vec![format!(
        "{}: {} tests, {} runs in {} commands over {} days; pass rate {:.0}%; {} flaky, {} newly failing, {} slow.",
        v["repo"].as_str().unwrap_or(""),
        t["tests"],
        t["runs"],
        t["commands"],
        v["window"]["days"],
        t["pass_rate"].as_f64().unwrap_or(1.0) * 100.0,
        t["flaky"],
        t["newly_failing"],
        t["slow"]
    )];
    let mut tests: Vec<&Json> = v["tests"].as_array().into_iter().flatten().collect();
    let rank = |x: &Json| (x["newly_failing"] == true) as u8 * 4 + (x["flaky"] == true) as u8 * 2 + (x["slow"] == true) as u8;
    tests.sort_by(|a, b| rank(b).cmp(&rank(a)).then(a["pass_rate"].as_f64().partial_cmp(&b["pass_rate"].as_f64()).unwrap_or(std::cmp::Ordering::Equal)));
    for x in tests.iter().take(40) {
        let mut flags = Vec::new();
        if x["newly_failing"] == true {
            flags.push("NEWLY FAILING".to_string());
        }
        if x["flaky"] == true {
            flags.push(format!("FLAKY ({})", x["flaky_why"].as_str().unwrap_or("")));
        }
        if x["slow"] == true {
            flags.push("SLOW".into());
        }
        out.push(format!(
            "{} [{}] {}/{} passed, p50 {}, p99 {}{}",
            x["test"].as_str().unwrap_or(""),
            x["runner"].as_str().unwrap_or(""),
            x["passed"],
            x["runs"],
            fmt_ms(x["p50_ms"].as_i64()),
            fmt_ms(x["p99_ms"].as_i64()),
            if flags.is_empty() { String::new() } else { format!("; {}", flags.join("; ")) }
        ));
    }
    if tests.len() > 40 {
        out.push(format!("… and {} more", tests.len() - 40));
    }
    out.join("\n")
}

/// `test_history`'s text for one test: its runs, newest first.
pub fn history_text(v: &Json) -> String {
    let mut out = vec![format!("{} [{}]", v["test"].as_str().unwrap_or(""), v["runner"].as_str().unwrap_or(""))];
    if v["flaky"] == true {
        out.push(format!("flaky: {}", v["flaky_why"].as_str().unwrap_or("")));
    }
    if v["newly_failing"] == true {
        out.push("newly failing: it passed before and fails now".into());
    }
    let names: HashMap<&str, &str> = v["agents"].as_array().into_iter().flatten().filter_map(|a| Some((a["id"].as_str()?, a["name"].as_str()?))).collect();
    for r in v["runs"].as_array().into_iter().flatten().take(30) {
        let agent = r["agent"].as_str().unwrap_or("");
        out.push(format!(
            "{} {} {} by {} on {}{}",
            r["ts"].as_str().unwrap_or(""),
            r["outcome"].as_str().unwrap_or(""),
            fmt_ms(r["ms"].as_i64()),
            names.get(agent).copied().unwrap_or(agent),
            r["commit"].as_str().map(|c| &c[..c.len().min(8)]).unwrap_or("no commit"),
            r["error"].as_str().map(|e| format!(": {}", one_line(e, 160))).unwrap_or_default()
        ));
    }
    out.join("\n")
}

fn repo_of(sup: &Supervisor, repo: Option<&str>) -> Result<String, ApiError> {
    match repo.filter(|r| !r.is_empty()) {
        Some(r) => Ok(crate::trail::repo_dir(Some(r)).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| r.to_string())),
        None => {
            // The repository most agents work in.
            let mut n: HashMap<String, usize> = HashMap::new();
            for a in sup.agent_rows() {
                *n.entry(a.repo).or_default() += 1;
            }
            n.into_iter().max_by_key(|(_, c)| *c).map(|(r, _)| r).ok_or_else(|| ApiError::bad("repo is required (no agent has run yet)"))
        }
    }
}

// ----------------------------------------------------------------- routes

#[derive(serde::Deserialize)]
struct TestsQ {
    repo: Option<String>,
    days: Option<i64>,
    test: Option<String>,
    agent: Option<String>,
}

async fn tests_route(State(app): State<App>, Query(q): Query<TestsQ>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || radar(&sup, q.repo.as_deref(), q.days.unwrap_or(7))).await?).into_response())
}

async fn history_route(State(app): State<App>, Query(q): Query<TestsQ>) -> ApiResult {
    let sup = app.sup.clone();
    let test = q.test.filter(|t| !t.is_empty()).ok_or_else(|| ApiError::bad("test is required"))?;
    Ok(AxJson(blocking(move || history(&sup, q.repo.as_deref(), &test)).await?).into_response())
}

async fn runs_route(State(app): State<App>, Query(q): Query<TestsQ>) -> ApiResult {
    let sup = app.sup.clone();
    let agent = q.agent.filter(|t| !t.is_empty()).ok_or_else(|| ApiError::bad("agent is required"))?;
    Ok(AxJson(blocking(move || agent_runs(&sup, &agent)).await?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_lines_report_times_and_failures() {
        let out = "running 3 tests\ntest totals::rounds ... ok <0.012s>\ntest totals::coupon ... FAILED <0.041s>\ntest totals::slow ... ignored\n\nfailures:\n\n---- totals::coupon stdout ----\nthread 'totals::coupon' panicked at tests/totals.rs:88:5:\nassertion `left == right` failed\n  left: 1799\n right: 1800\n\nfailures:\n    totals::coupon\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.52s\n";
        let p = parse("cargo test totals", out);
        assert_eq!(p.runner, "cargo");
        assert_eq!(p.tests.len(), 3);
        assert_eq!((p.tests[0].name.as_str(), p.tests[0].outcome, p.tests[0].ms), ("totals::rounds", "passed", Some(12)));
        assert_eq!((p.tests[1].outcome, p.tests[1].ms), ("failed", Some(41)));
        assert!(p.tests[1].error.as_deref().unwrap().contains("left: 1799"));
        assert_eq!(p.tests[2].outcome, "skipped");
        assert_eq!(p.total_ms, Some(520));
        let p = parse("cargo test", "test a::b ... ok\ntest a::c ... FAILED\n");
        assert_eq!(p.tests.iter().map(|t| t.ms).collect::<Vec<_>>(), vec![None, None]);
        let p = parse("cargo nextest run", "    PASS [   0.012s] shop tests::a\n    FAIL [   1.500s] shop tests::b\n");
        assert_eq!(p.runner, "nextest");
        assert_eq!((p.tests[1].name.as_str(), p.tests[1].outcome, p.tests[1].ms), ("shop tests::b", "failed", Some(1500)));
    }

    #[test]
    fn pytest_verbose_summary_and_durations() {
        let out = "tests/test_csv.py::test_bom PASSED                     [ 33%]\ntests/test_csv.py::test_sep FAILED                     [ 66%]\ntests/test_csv.py::test_skip SKIPPED (no db)           [100%]\n\n=================================== FAILURES ===================================\n___________________________________ test_sep ___________________________________\n\n    def test_sep():\n>       assert split('a;b') == ['a', 'b']\nE       AssertionError: assert ['a;b'] == ['a', 'b']\n\ntests/test_csv.py:12: AssertionError\n============================= slowest durations ==============================\n0.52s call     tests/test_csv.py::test_bom\n=========================== short test summary info ============================\nFAILED tests/test_csv.py::test_sep - AssertionError: assert ['a;b'] == ['a', 'b']\n==================== 1 failed, 1 passed, 1 skipped in 0.61s ====================\n";
        let p = parse("pytest tests -v --durations=5", out);
        assert_eq!(p.runner, "pytest");
        assert_eq!(p.tests.len(), 3);
        assert_eq!((p.tests[0].outcome, p.tests[0].ms), ("passed", Some(520)));
        assert_eq!(p.tests[1].outcome, "failed");
        assert!(p.tests[1].error.as_deref().unwrap().contains("AssertionError"));
        assert_eq!(p.tests[2].outcome, "skipped");
        assert_eq!(p.total_ms, Some(610));
        // Without -v: the failures from the summary.
        let p = parse("python -m pytest -q", "..F.\nFAILED tests/a.py::test_x - assert 1 == 2\n1 failed, 3 passed in 0.10s\n");
        assert_eq!(p.tests.len(), 1);
        assert_eq!((p.tests[0].name.as_str(), p.tests[0].outcome), ("tests/a.py::test_x", "failed"));
    }

    #[test]
    fn jest_vitest_and_go() {
        let jest = "PASS web/src/cart/CartLine.test.tsx\n  CartLine\n    ✓ shows the coupon line (5 ms)\n    ✕ rounds (3 ms)\n    ○ skipped later\n\n  ● CartLine › rounds\n\n    expect(received).toBe(expected)\n    Expected: 1800\n\nTest Suites: 1 failed, 1 total\n";
        let p = parse("npx jest web/src/cart", jest);
        assert_eq!(p.runner, "jest");
        assert_eq!(p.tests.len(), 3);
        assert_eq!((p.tests[0].name.as_str(), p.tests[0].ms), ("web/src/cart/CartLine.test.tsx › CartLine › shows the coupon line", Some(5)));
        assert_eq!(p.tests[1].outcome, "failed");
        assert!(p.tests[1].error.as_deref().unwrap().contains("Expected: 1800"));
        assert_eq!(p.tests[2].outcome, "skipped");
        let vitest = " ✓ src/search/facets.test.ts > facets > renders brands 4ms\n × src/search/facets.test.ts > facets > keeps price bounds 12ms\n ✓ src/search/other.test.ts (3 tests) 9ms\n FAIL  src/search/facets.test.ts > facets > keeps price bounds\n";
        let p = parse("npx vitest run", vitest);
        assert_eq!(p.runner, "vitest");
        assert_eq!(p.tests.len(), 2);
        assert_eq!((p.tests[1].name.as_str(), p.tests[1].outcome, p.tests[1].ms), ("src/search/facets.test.ts › facets › keeps price bounds", "failed", Some(12)));
        let go = "=== RUN   TestVAT\n--- PASS: TestVAT (0.00s)\n=== RUN   TestFormat\n    format_test.go:12: got 1,00 want 1.00\n--- FAIL: TestFormat (0.01s)\nFAIL\nFAIL\tshop/pricing\t0.012s\n";
        let p = parse("go test ./pricing/... -v", go);
        assert_eq!(p.runner, "go");
        assert_eq!(p.tests.len(), 2);
        assert_eq!((p.tests[1].name.as_str(), p.tests[1].outcome, p.tests[1].ms), ("shop/pricing › TestFormat", "failed", Some(10)));
        assert!(p.tests[1].error.as_deref().unwrap().contains("format_test.go:12"));
        // Nothing a parser knows.
        assert_eq!(parse("make check", "all good\n").runner, "other");
    }

    fn row(outcome: &str, commit: &str, tree: &str, ts: i64) -> Row {
        Row { test: "t".into(), run: format!("r{ts}"), agent: "a".into(), commit: Some(commit.into()), tree: tree.into(), outcome: outcome.into(), ms: Some(10), own_ms: true, ts, runner: "cargo".into(), command: "cargo test".into(), exit_code: Some(0), seq: None, error: None }
    }

    #[test]
    fn verdicts() {
        // Same commit, same code, both outcomes: flaky.
        let rs = [row("passed", "c1", "clean", 1), row("failed", "c1", "clean", 2), row("passed", "c1", "clean", 3)];
        let v = verdict(&rs.iter().collect::<Vec<_>>());
        assert!(v.flaky && v.flaky_why.unwrap().contains("same code"));
        assert!(!v.newly_failing);
        // Code changed between them, one flip: an agent broke it; newly failing.
        let rs = [row("passed", "c1", "clean", 1), row("failed", "c1", "d1", 2)];
        let v = verdict(&rs.iter().collect::<Vec<_>>());
        assert!(!v.flaky && v.newly_failing);
        // Flipping twice on one commit within a day, the code changing: flaky.
        let rs = [row("passed", "c1", "a", 1), row("failed", "c1", "b", 2), row("passed", "c1", "c", 3)];
        assert!(verdict(&rs.iter().collect::<Vec<_>>()).flaky);
        // A new commit between them: not.
        let rs = [row("passed", "c1", "a", 1), row("failed", "c2", "b", 2), row("passed", "c3", "c", 3)];
        assert!(!verdict(&rs.iter().collect::<Vec<_>>()).flaky);
        assert_eq!(percentile(&[1, 2, 3, 4], 0.5), Some(2));
        assert_eq!(percentile(&[1, 2, 3, 4], 0.99), Some(4));
        assert!(is_slow(Some(100), Some(6000)) && is_slow(Some(300), Some(1200)) && !is_slow(Some(400), Some(900)));
    }
}
