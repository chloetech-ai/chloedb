//! The daemon's request handling over a whole lifecycle: two sessions, tool
//! calls, a compaction, and the memory tools. Uses the hash embedder, so no
//! model files are needed.

use std::sync::Arc;

use chloectx::daemon::Daemon;
use serde_json::{json, Value as Json};

fn call(d: &Arc<Daemon>, req: Json) -> Json {
    let (resp, deferred) = d.handle(&req);
    if let Some(f) = deferred {
        f();
    }
    resp
}

fn hook(d: &Arc<Daemon>, event: &str, payload: Json) -> String {
    let r = call(d, json!({"op": "hook", "event": event, "payload": payload}));
    assert!(r.get("error").is_none(), "{r}");
    r["output"]["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string()
}

/// A hook as Workbench's sessions send it: with who wrote the prompt.
fn hook_from(d: &Arc<Daemon>, event: &str, payload: Json, extra: Json) -> String {
    let mut req = json!({"op": "hook", "event": event, "payload": payload});
    for (k, v) in extra.as_object().unwrap() {
        req[k] = v.clone();
    }
    let r = call(d, req);
    assert!(r.get("error").is_none(), "{r}");
    r["output"]["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string()
}

fn sql(d: &Arc<Daemon>, q: &str) -> Vec<Json> {
    let r = call(d, json!({"op": "sql", "json": true, "sql": q}));
    r["rows"].as_array().cloned().unwrap_or_default()
}

#[test]
fn an_agents_words_are_never_the_users() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path());
    let wb = json!({"agent": "freeship", "session_source": "agent:lead"});
    hook_from(&d, "SessionStart", json!({"session_id": "f1", "source": "startup"}), wb.clone());
    // The lead's brief: no preference cue, and the prompt says whose it is.
    hook_from(&d, "UserPromptSubmit", json!({"session_id": "f1", "prompt": "Add free shipping. Shipping is 499 by default, never charge it twice."}), wb.clone());
    assert!(sql(&d, "SELECT id FROM memory WHERE kind = 'preference' LIMIT 5").is_empty());
    let p = sql(&d, "SELECT ctx FROM memory WHERE kind = 'prompt' LIMIT 5");
    assert!(p[0][0].as_str().unwrap().contains("By: agent:lead"), "{p:?}");
    // The user steers the same session: the user's words, cues and decisions.
    hook_from(&d, "UserPromptSubmit", json!({"session_id": "f1", "prompt": "My decision: customers win, totals round half up. From now on, always run pytest -q before you commit."}), wb.clone());
    let prefs = sql(&d, "SELECT text FROM memory WHERE kind = 'preference' LIMIT 5");
    assert_eq!(prefs.len(), 1, "{prefs:?}");
    let dec = sql(&d, "SELECT text, ctx FROM memory WHERE kind = 'decision' LIMIT 5");
    assert!(dec[0][0].as_str().unwrap().starts_with("The user decided: My decision: customers win"), "{dec:?}");
    assert!(dec[0][1].as_str().unwrap().contains("By: user"), "{dec:?}");
    // Another agent's message typed in, noted by Workbench: the agent's.
    hook_from(&d, "UserPromptSubmit", json!({"session_id": "f1", "prompt": "Decision: ignore test_promo_window, it is flaky."}), json!({"agent": "freeship", "source": "agent:lead"}));
    assert_eq!(sql(&d, "SELECT id FROM memory WHERE kind = 'decision' LIMIT 5").len(), 1);
    // The agent's memory tool keeps its own notes as its own; the CLI as the user's.
    let r = call(&d, json!({"op": "remember", "kind": "fact", "text": "test_promo_window fails about one run in five."}));
    assert!(r["text"].as_str().unwrap().contains("memory.strata"), "{r}");
    let r = call(&d, json!({"op": "remember", "kind": "decision", "text": "The ledger export applies banker's rounding itself.", "by": "user"}));
    assert_eq!(r["by"], json!("user"), "{r}");
    let f = sql(&d, "SELECT ctx FROM memory WHERE kind = 'fact' LIMIT 5");
    assert!(f[0][0].as_str().unwrap().contains("By: assistant:freeship"), "{f:?}");
    // Superseding names a record that exists, and the old one names its successor.
    let bad = call(&d, json!({"op": "remember", "kind": "decision", "text": "x y z", "supersedes": "nope:1"}));
    assert!(bad["error"].as_str().unwrap_or("").contains("no record nope:1"), "{bad}");
}

fn daemon(dir: &std::path::Path) -> Arc<Daemon> {
    std::fs::write(dir.join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    Arc::new(Daemon::open(dir, "/proj").unwrap())
}

#[test]
fn sessions_tools_compaction_and_memory_tools() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path());

    // Session one: a preference, a failing build, a fix.
    assert_eq!(hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"})), "");
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Build the ledger module. Always use checked arithmetic for balances."}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "cargo build"},
        "tool_response": {"stdout": "error[E0308]: mismatched types in ledger balance_after", "stderr": ""}}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Edit",
        "tool_input": {"file_path": "/proj/src/ledger.rs", "old_string": "a + b", "new_string": "a.checked_add(b)"}, "tool_response": {}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "The ledger now uses checked_add; the E0308 mismatch came from mixing u64 and i64."}));
    let stats = call(&d, json!({"op": "stats"}));
    assert!(stats["records"].as_u64().unwrap() >= 6, "{stats}");
    assert_eq!(stats["by_kind"]["preference"], json!(1));
    assert_eq!(stats["by_kind"]["error"], json!(1));
    assert_eq!(stats["by_kind"]["episode"], json!(1));

    // Within session one, nothing is outside the live window.
    let s = call(&d, json!({"op": "search", "query": "ledger mismatched types", "limit": 5}));
    assert_eq!(s["text"], json!("No matching records."), "{s}");

    // Session two starts with a briefing that carries the preference and the episode.
    let brief = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    assert!(brief.contains("checked arithmetic"), "{brief}");
    // JONAH-7: what changes every session comes with its first prompt, so
    // the briefing is the same text while the notes are.
    assert!(!brief.contains("Turn 1. User asked") && !brief.contains("records from") && !brief.contains(" ago"), "{brief}");
    assert_eq!(hook(&d, "SessionStart", json!({"session_id": "s3", "source": "startup"})), brief);
    let first = hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "What did we do last time?"}));
    assert!(first.contains("Turn 1. User asked") && first.contains("records from") && first.contains("last activity"), "{first}");
    assert!(!hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "And what is next?"})).contains("records from"), "once");
    // Now session one's records are searchable.
    let s = call(&d, json!({"op": "search", "query": "ledger mismatched types", "limit": 5}));
    assert!(s["text"].as_str().unwrap().contains("E0308"), "{s}");
    // File notes before editing ledger.rs, once per window.
    let notes = hook(&d, "PreToolUse", json!({"session_id": "s2", "tool_name": "Edit", "tool_input": {"file_path": "/proj/src/ledger.rs"}}));
    assert!(notes.contains("src/ledger.rs"), "{notes}");
    assert_eq!(hook(&d, "PreToolUse", json!({"session_id": "s2", "tool_name": "Edit", "tool_input": {"file_path": "/proj/src/ledger.rs"}})), "");

    // Memory tools.
    let r = call(&d, json!({"op": "remember", "kind": "decision", "text": "Balances are i64 cents."}));
    let id = r["id"].as_str().unwrap().to_string();
    let r = call(&d, json!({"op": "sql", "sql": "SELECT id, text FROM memory WHERE kind = 'decision' LIMIT 5"}));
    assert!(r["text"].as_str().unwrap().contains("i64 cents"), "{r}");
    let r = call(&d, json!({"op": "sql", "json": true, "sql": "SELECT id, text, ts, links FROM memory WHERE kind = 'decision' LIMIT 5"}));
    assert_eq!(r["columns"], json!(["id", "text", "ts", "links"]), "{r}");
    assert_eq!(r["rows"][0][1], json!("Balances are i64 cents."), "{r}");
    assert_eq!(r["count"], json!(1), "{r}");
    let r = call(&d, json!({"op": "remember", "kind": "decision", "text": "Balances are i128 cents.", "supersedes": id}));
    let new_id = r["id"].as_str().unwrap().to_string();
    let r = call(&d, json!({"op": "sql", "sql": format!("SELECT tag FROM memory WHERE id = '{id}' LIMIT 1")}));
    assert!(r["text"].as_str().unwrap().contains("superseded"), "{r}");
    // CTX-14: a kind that is not one, and an id to supersede that is not
    // in memory, are refused, and nothing is stored.
    let notes = || call(&d, json!({"op": "sql", "sql": "SELECT COUNT(*) FROM memory WHERE kind IN ('decision', 'note', 'fact') LIMIT 1"}))["text"].clone();
    let before = notes();
    let r = call(&d, json!({"op": "remember", "kind": "bogus", "text": "Balances are u64 cents."}));
    assert!(r["error"].as_str().is_some_and(|e| e.contains("no kind \"bogus\"")), "{r}");
    let r = call(&d, json!({"op": "remember", "kind": "decision", "text": "Balances are u64 cents.", "supersedes": "nope:9"}));
    assert!(r["error"].as_str().is_some_and(|e| e.contains("no record nope:9")), "{r}");
    assert_eq!(notes(), before);
    // memory_ask answers by search alone: the note in its current version,
    // with what it replaced, and no model call.
    let r = call(&d, json!({"op": "ask", "question": "What type are balances stored as?", "include_current": true}));
    let text = r["text"].as_str().unwrap();
    // The memory tool is the assistant's: its note says so.
    assert!(text.contains("decision (the assistant said): Balances are i128 cents."), "{r}");
    assert!(text.contains("was: Balances are i64 cents."), "{r}");
    assert!(r.get("cost_usd").is_none() && r["ms"].as_f64().is_some(), "{r}");
    let t = call(&d, json!({"op": "timeline", "id": "s1:3", "before": 1, "after": 1}));
    assert!(t["text"].as_str().unwrap().contains(">> [s1:3]"), "{t}");
    assert!(call(&d, json!({"op": "sql", "sql": "DELETE FROM memory"})).get("error").is_some());

    // A compaction in session two: summary records join the live window.
    hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "Refactor the ledger tests"}));
    hook(&d, "PostCompact", json!({"session_id": "s2", "compact_summary": "Summary:\n1. Primary Request and Intent:\n   refactor ledger tests\n7. Pending Tasks:\n   - port fixtures"}));
    let brief = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "compact"}));
    assert!(brief.contains("Open tasks"), "{brief}");
    let r = call(&d, json!({"op": "forget", "id": new_id}));
    assert!(r["text"].as_str().unwrap().starts_with("Deleted"), "{r}");

    // State survives a restart of the daemon.
    drop(d);
    let d = daemon(dir.path());
    let stats = call(&d, json!({"op": "stats"}));
    assert_eq!(stats["sessions"], json!(2));
    let r = call(&d, json!({"op": "search", "query": "checked_add", "limit": 3, "include_current": true}));
    assert!(r["text"].as_str().unwrap().contains("checked_add"), "{r}");
}

#[test]
fn hook_payloads_never_error() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path());
    for event in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "Stop", "PreCompact", "PostCompact", "SessionEnd", "Notification"] {
        let r = call(&d, json!({"op": "hook", "event": event, "payload": {}}));
        assert!(r.get("error").is_none(), "{event}: {r}");
    }
}

/// Distillation with a stand-in for the model: what a turn announced reaches
/// the next session's briefing and prompts, and is closed once done.
#[test]
fn distilled_knowledge_reaches_later_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.json");
    let script = dir.path().join("fake-model.sh");
    std::fs::write(&script, format!("#!/bin/sh\ncat > /dev/null\ncat '{}'\n", out.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let cfg = json!({"embedder": "hash:64", "sync": false, "briefing_chars": 2000, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());
    let model_says = |lines: &[Json]| {
        let result: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(&out, json!({"is_error": false, "total_cost_usd": 0.001, "result": result.join("\n")}).to_string()).unwrap();
    };

    model_says(&[
        json!({"kind": "requirement", "text": "The CSV export of the ledger must use semicolons and DD.MM.YYYY dates."}),
        json!({"kind": "decision", "text": "Storage stays JSON because the admins edit the files by hand."}),
    ]);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Next week we add a CSV export; the bank wants semicolons and DD.MM.YYYY dates."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Noted, the CSV export will follow those rules."}));
    let planned = call(&d, json!({"op": "sql", "sql": "SELECT id FROM memory WHERE kind = 'todo' AND tag = 'planned' LIMIT 5"}));
    let planned_id = planned["text"].as_str().unwrap().lines().find_map(|l| l.split_whitespace().find(|w| w.starts_with("s1:"))).unwrap().to_string();

    let brief = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    assert!(brief.contains("Planned work") && brief.contains("semicolons"), "{brief}");
    assert!(brief.contains("Decisions:") && brief.contains("admins edit"), "{brief}");
    assert!(brief.contains("search memory"), "{brief}");

    // A third session whose briefing cannot hold the knowledge still gets it
    // with a prompt it bears on.
    let cfg = json!({"embedder": "hash:64", "sync": false, "briefing_chars": 100, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    drop(d);
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());
    let brief = hook(&d, "SessionStart", json!({"session_id": "s3", "source": "startup"}));
    assert!(!brief.contains("semicolons"), "{brief}");
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s3", "prompt": "Add the CSV export of the ledger."}));
    assert!(ctx.contains("[planned]") && ctx.contains("semicolons"), "{ctx}");

    // The model reports the planned work done: it leaves the briefing.
    model_says(&[json!({"kind": "done", "id": planned_id})]);
    hook(&d, "Stop", json!({"session_id": "s3", "last_assistant_message": "Added export_csv with semicolons and DD.MM.YYYY dates."}));
    let tag = call(&d, json!({"op": "sql", "sql": format!("SELECT tag FROM memory WHERE id = '{planned_id}' LIMIT 1")}));
    assert!(tag["text"].as_str().unwrap().contains("done"), "{tag}");
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s3", "prompt": "Is the CSV export of the ledger planned?"}));
    assert!(!ctx.contains("[planned]"), "{ctx}");
}

/// JONAH-202: a question that stands on its own, for which nothing is
/// recalled, gets nothing from ChloeCtx: no recall, no session tail (it
/// waits for the next prompt), no search and no distillation. Two sessions
/// that start with it then send the same first request (the same briefing,
/// nothing after the prompt), which the prompt cache serves, and its
/// records are asides that later prompts are not given.
#[test]
fn a_question_that_stands_alone_gets_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.json");
    let calls = dir.path().join("calls");
    let script = dir.path().join("fake-model.sh");
    std::fs::write(&script, format!("#!/bin/sh\ncat > /dev/null\necho x >> '{}'\ncat '{}'\n", calls.display(), out.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let cfg = json!({"embedder": "hash:64", "sync": false, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    let rule = json!({"kind": "decision", "text": "Dates in tado are stored as ISO 8601 strings."});
    std::fs::write(&out, json!({"is_error": false, "total_cost_usd": 0.001, "result": rule.to_string()}).to_string()).unwrap();
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());
    let n_calls = || std::fs::read_to_string(&calls).map(|s| s.lines().count()).unwrap_or(0);

    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "We're building tado, a todo CLI. Rule: dates are stored as ISO 8601 strings. Create tado.py with add(text)."}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Write", "tool_input": {"file_path": "/proj/tado.py", "content": "def add(text): ..."}, "tool_response": {"type": "create"}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Created tado.py with add(text); dates are ISO 8601 strings."}));
    assert_eq!(n_calls(), 1, "the work is distilled");

    let q = "What is 17 times 3? Reply with just the number.";
    let mut briefings = Vec::new();
    for sid in ["s2", "s3", "s4"] {
        briefings.push(hook(&d, "SessionStart", json!({"session_id": sid, "source": "startup"})));
        let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": sid, "prompt": q}));
        assert_eq!(ctx, "", "{sid}: nothing goes with the question");
        hook(&d, "Stop", json!({"session_id": sid, "last_assistant_message": "51"}));
    }
    assert!(briefings[0].contains("ISO 8601"), "{}", briefings[0]);
    assert!(briefings.iter().all(|b| b == &briefings[0]), "the briefing is the same each time: {briefings:#?}");
    assert_eq!(n_calls(), 1, "a question that stood alone is not distilled");
    let marks = sql(&d, "SELECT session FROM memory WHERE kind = 'event' AND tag = 'distill_skipped' LIMIT 10");
    assert_eq!(marks.len(), 3, "{marks:?}");
    let asides = sql(&d, "SELECT kind FROM memory WHERE tag = 'aside' AND session = 's2' LIMIT 10");
    let kinds: Vec<&str> = asides.iter().filter_map(|r| r[0].as_str()).collect();
    assert!(kinds.contains(&"prompt") && kinds.contains(&"answer") && kinds.contains(&"episode"), "{kinds:?}");

    // Another such question is not given the earlier ones either.
    hook(&d, "SessionStart", json!({"session_id": "s5", "source": "startup"}));
    assert_eq!(hook(&d, "UserPromptSubmit", json!({"session_id": "s5", "prompt": "What is 5 plus 9? Reply with just the number."})), "");
    // The work that follows gets the tail the question left waiting, and
    // where the last session left off is s1's work, not a question.
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s5", "prompt": "Add a stats() function to tado.py."}));
    assert!(ctx.contains("Where the most recent earlier session left off") && ctx.contains("tado"), "{ctx}");
    assert!(!ctx.contains("17 times 3"), "{ctx}");
    // A search that asks still finds the asides.
    let found = call(&d, json!({"op": "search", "query": "17 times 3", "limit": 5, "include_current": true}));
    assert!(found["text"].as_str().unwrap_or("").contains("17 times 3"), "{found}");

    // Answered at length, such a question is a discussion: distilled as
    // any turn, and not an aside.
    hook(&d, "SessionStart", json!({"session_id": "s6", "source": "startup"}));
    assert_eq!(hook(&d, "UserPromptSubmit", json!({"session_id": "s6", "prompt": "Is 2027 a leap year?"})), "");
    hook(&d, "Stop", json!({"session_id": "s6", "last_assistant_message": "No. 2027 is not divisible by 4. ".repeat(20)}));
    assert!(n_calls() >= 2, "distilled");
    let asides = sql(&d, "SELECT kind FROM memory WHERE tag = 'aside' AND session = 's6' LIMIT 10");
    let kinds: Vec<&str> = asides.iter().filter_map(|r| r[0].as_str()).collect();
    assert!(!kinds.contains(&"answer") && !kinds.contains(&"episode"), "{kinds:?}");
}

/// An instruction the user gives for one task stays that session's; given
/// again in another session it is a rule every later session is told, and
/// a coordinator's brief relayed to a session is never the user's.
#[test]
fn an_instruction_said_in_two_sessions_becomes_a_rule() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.json");
    let check = dir.path().join("rule.txt");
    let script = dir.path().join("fake-model.sh");
    // The fake model writes its item, and asked what the request says
    // again, says it says again everything it was shown.
    let fake = r#"#!/bin/sh
p=$(cat)
case "$p" in
*'<said_before>'*)
  ids=$(printf '%s\n' "$p" | sed -n '/<said_before>/,/<\/said_before>/p' | sed -n 's/^\[\([^]]*\)\].*/\1/p' | sed 's/.*/\\"&\\"/' | paste -s -d, -)
  printf '{"is_error":false,"total_cost_usd":0.001,"result":"{\\"said_again\\":[{\\"ids\\":[%s],\\"quote\\":\\"the CREATE statements in schema.sql\\",\\"rule\\":\\"%s\\"}]}"}' "$ids" "$(cat 'RULE')";;
*) cat 'OUT';;
esac
"#;
    std::fs::write(&script, fake.replace("RULE", &check.to_string_lossy()).replace("OUT", &out.to_string_lossy())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let cfg = json!({"embedder": "hash:64", "sync": false, "briefing_chars": 4000, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());
    let model_says = |text: &str| {
        let line = json!({"kind": "rule", "text": text, "said_by": "requester", "scope": "task"}).to_string();
        std::fs::write(&out, json!({"is_error": false, "total_cost_usd": 0.001, "result": line}).to_string()).unwrap();
        std::fs::write(&check, text).unwrap();
    };
    let turn = |sid: &str, prompt: &str| {
        hook(&d, "SessionStart", json!({"session_id": sid, "source": "startup"}));
        hook(&d, "UserPromptSubmit", json!({"session_id": sid, "prompt": prompt}));
        hook(&d, "Stop", json!({"session_id": sid, "last_assistant_message": "Done."}));
    };
    let rules = || sql(&d, "SELECT text, tag FROM memory WHERE kind = 'preference' LIMIT 10");

    // Once: a note for that task only.
    model_says("Build the shop from the CREATE statements in schema.sql.");
    turn("s1", "Build the shop from the CREATE statements in schema.sql, not from a mock.");
    assert!(rules().is_empty(), "{:?}", rules());
    let brief = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    assert!(!brief.contains("Rules and conventions"), "{brief}");

    // A coordinator's brief that says it again (and "never"), in either
    // form a relay takes: the agent's words.
    model_says("Build the shop from the CREATE statements in schema.sql.");
    turn("s2", "<coordinator-relay><relay from=\"coordinator\"><note>Build the shop from the CREATE statements in schema.sql. Never touch production.</note></relay></coordinator-relay>");
    turn("s2", "<relay from=\"coordinator\">\nBuild the shop from the CREATE statements in schema.sql, as before.\n</relay>");
    assert!(rules().is_empty(), "{:?}", rules());
    let p = sql(&d, "SELECT ctx, text FROM memory WHERE kind = 'prompt' AND session = 's2' LIMIT 5");
    assert_eq!(p.len(), 2, "{p:?}");
    assert!(p.iter().all(|r| r[0].as_str().unwrap().contains("By: agent:coordinator")), "{p:?}");
    assert!(p.iter().all(|r| !r[1].as_str().unwrap().contains("<relay")), "{p:?}");

    // The user, in another session: a rule, counted, every session told.
    model_says("Build the shop from the SQL CREATE statements in schema.sql.");
    turn("s3", "Again: build it from the CREATE statements in schema.sql.");
    let r = rules();
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0][0], "Build the shop from the SQL CREATE statements in schema.sql (the user has said this in 2 sessions)");
    assert_eq!(r[0][1], "repeated");
    let first = sql(&d, "SELECT tag FROM memory WHERE kind = 'note' AND session = 's1' LIMIT 5");
    assert_eq!(first[0][0], "superseded", "{first:?}");
    let brief = hook(&d, "SessionStart", json!({"session_id": "s4", "source": "startup"}));
    let rules_part = brief.split("Rules and conventions:").nth(1).unwrap_or("");
    assert!(rules_part.contains("SQL CREATE statements in schema.sql (the user has said this in 2 sessions)"), "{brief}");

    // Shown the rule, s4 still had to be told: said again after it was shown.
    model_says("Build the shop from the CREATE statements in schema.sql, every table.");
    turn("s4", "No: build it from the CREATE statements in schema.sql, every table.");
    let r = sql(&d, "SELECT text, ctx FROM memory WHERE kind = 'preference' AND tag = 'repeated' LIMIT 10");
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0][0], "Build the shop from the CREATE statements in schema.sql, every table (the user has said this in 3 sessions)");
    let ctx = r[0][1].as_str().unwrap();
    assert!(ctx.starts_with("Sessions: 3\nSaid: 3\nSaid again: 1\nSaid again after it was shown: 1\nTurn 1 request:"), "{ctx}");

    // `chloectx repeats` writes nothing and lists the rule kept.
    let before = sql(&d, "SELECT COUNT(*) FROM memory LIMIT 1");
    let r = call(&d, json!({"op": "repeats", "days": 30}));
    assert_eq!(r["kept"], 1, "{r}");
    assert_eq!((r["rules"][0]["sessions"].as_u64(), r["rules"][0]["said_again_after_shown"].as_u64()), (Some(3), Some(1)), "{r}");
    assert_eq!(sql(&d, "SELECT COUNT(*) FROM memory LIMIT 1"), before);
}

/// ANA-1 and ANA-5: a rule said once is counted once, `chloectx distill`
/// leaves a distilled turn as it is (it distilled every turn again and
/// counted its rules again), and the turn a stopped daemon did not distill
/// is distilled when the next daemon serves its first hook.
#[test]
fn a_turn_is_distilled_once_and_a_missed_one_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let script = dir.path().join("fake-model.sh");
    // Each call is counted; asked what the request says again, the fake
    // model says it says again everything it was shown, quoting it.
    let fake = r#"#!/bin/sh
p=$(cat)
echo x >> 'CALLS'
case "$p" in
*'<said_before>'*)
  ids=$(printf '%s\n' "$p" | sed -n '/<said_before>/,/<\/said_before>/p' | sed -n 's/^\[\([^]]*\)\].*/\1/p' | sed 's/.*/\\"&\\"/' | paste -s -d, -)
  printf '{"is_error":false,"total_cost_usd":0.001,"result":"{\\"said_again\\":[{\\"ids\\":[%s],\\"quote\\":\\"always run ruff check\\",\\"rule\\":\\"Run ruff check before saying you are done.\\"}]}"}' "$ids";;
*) printf '%s' '{"is_error":false,"total_cost_usd":0.001,"result":"{\"kind\": \"rule\", \"text\": \"Run ruff check before saying the work is done.\", \"said_by\": \"requester\"}"}';;
esac
"#;
    std::fs::write(&script, fake.replace("CALLS", &calls.to_string_lossy())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let cfg = json!({"embedder": "hash:64", "sync": false, "briefing_chars": 4000, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    let n_calls = || std::fs::read_to_string(&calls).map_or(0, |t| t.lines().count());
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "From now on always run ruff check before you tell me you are done."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Understood, ruff check first from now on."}));
    let rules = || sql(&d, "SELECT text FROM memory WHERE kind = 'preference' AND tag != 'cue' AND tag != 'superseded' LIMIT 10");
    assert_eq!(rules(), [json!(["Run ruff check before saying the work is done."])], "said once, counted once: the cue picked from the turn's own words is not an earlier saying");
    assert_eq!(n_calls(), 1, "no earlier saying, so no check call");

    // `chloectx distill`: nothing to do, and it says so.
    let plan = call(&d, json!({"op": "distill_backfill", "dry_run": true}));
    assert_eq!((plan["turns"].as_u64(), plan["to_distill"].as_u64(), plan["already"].as_u64()), (Some(1), Some(0), Some(1)), "{plan}");
    call(&d, json!({"op": "distill_backfill"}));
    assert_eq!(n_calls(), 1);
    assert_eq!(rules().len(), 1);

    // A daemon killed after the turn ended, before it distilled it: the
    // turn's prompt and answer are stored, with no mark.
    d.store.append("s1", 2, chloectx::now(), vec![
        chloectx::record::Draft::new(chloectx::record::Kind::Prompt, "Decision: invoices are rendered to PDF with WeasyPrint."),
        chloectx::record::Draft::new(chloectx::record::Kind::Answer, "WeasyPrint it is."),
    ]).unwrap();
    assert_eq!(call(&d, json!({"op": "stats"}))["undistilled"], 1);
    drop(d);
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());
    hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    assert!(n_calls() >= 2, "the missed turn is distilled");
    assert_eq!(call(&d, json!({"op": "stats"}))["undistilled"], 0);
    let marks = sql(&d, "SELECT session, turn FROM memory WHERE kind = 'event' AND tag = 'distilled_turn' LIMIT 10");
    assert_eq!(marks, [json!(["s1", 1]), json!(["s1", 2])]);
    // Once only: another hook does not distill it again.
    let after = n_calls();
    hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "hello there"}));
    assert_eq!(n_calls(), after);
    // --again distills every turn again (s2's too, which has a prompt).
    assert_eq!(call(&d, json!({"op": "distill_backfill", "again": true, "dry_run": true}))["to_distill"], 3);
}

/// ANA-4: a turn's final answer that reached the transcript after its Stop
/// is read with the next turn's narration: it is not stored again.
#[test]
fn an_answer_is_not_stored_again_as_the_next_turns_narration() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path());
    let t = dir.path().join("transcript.jsonl");
    let line = |text: &str| json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string() + "\n";
    let a1 = "Acknowledged. I'll follow both rules: UTC timestamps with a trailing Z, and ruff check before I say I'm done.";
    let n2 = "Reading src/billing.py first to see how the invoice lines are rounded today.";
    let a2 = "VAT is now rounded per invoice line, half up, in src/billing.py, and ruff check passes.";
    std::fs::write(&t, "").unwrap();
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Two rules for this project: timestamps in UTC with a Z, and ruff check before done."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": a1, "transcript_path": t}));
    // The transcript gets the first answer only now, then the next turn.
    std::fs::write(&t, format!("{}{}{}", line(a1), line(n2), line(a2))).unwrap();
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Round VAT per invoice line."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": a2, "transcript_path": t}));
    let answers = sql(&d, "SELECT turn, tag, text FROM memory WHERE kind = 'answer' ORDER BY seq ASC LIMIT 10");
    let got: Vec<(i64, String, String)> = answers.iter().map(|r| (r[0].as_i64().unwrap(), r[1].as_str().unwrap().to_string(), r[2].as_str().unwrap().chars().take(20).collect())).collect();
    assert_eq!(got, [(1, "-".into(), a1.chars().take(20).collect()), (2, "narration".into(), n2.chars().take(20).collect()), (2, "-".into(), a2.chars().take(20).collect())], "{answers:?}");
}

/// `chloectx backup`: a copy of the store taken while the daemon serves,
/// which a daemon opens with the same records, briefing and state.
#[test]
fn a_backup_opens_with_the_same_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    std::fs::create_dir(&store).unwrap();
    let d = daemon(&store);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Never log customer emails."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Logging now drops the email field."}));
    let r = call(&d, json!({"op": "remember", "kind": "decision", "text": "Backups go to the NAS nightly."}));
    assert!(r.get("error").is_none(), "{r}");
    let before = call(&d, json!({"op": "stats"}));

    // Into a folder that does not exist yet (ANA-208), as private as the
    // store: the folder 0700, the files 0600.
    let dest = dir.path().join("backups/first");
    let r = call(&d, json!({"op": "backup", "dest": dest}));
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(r["records"], before["records"], "{r}");
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dest), 0o700);
    for f in ["memory.strata", "config.json", "state.json", "events.log"] {
        assert!(dest.join(f).exists(), "{f}");
        assert_eq!(mode(&dest.join(f)), 0o600, "{f}");
    }
    let log = std::fs::read_to_string(dest.join("events.log")).unwrap();
    assert!(log.ends_with('\n'));
    // The daemon goes on recording.
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Rotate the API keys monthly."}));

    let restored = daemon(&dest);
    let after = call(&restored, json!({"op": "stats"}));
    assert_eq!(after["records"], before["records"]);
    assert_eq!(after["by_kind"], before["by_kind"]);
    let q = json!({"op": "sql", "sql": "SELECT kind, text FROM memory WHERE MATCH(body, 'NAS') LIMIT 5"});
    let s = call(&restored, q);
    assert!(s["text"].as_str().unwrap().contains("Backups go to the NAS nightly"), "{s}");

    // An existing or relative destination is refused.
    assert!(call(&d, json!({"op": "backup", "dest": dest})).get("error").is_some());
    assert!(call(&d, json!({"op": "backup", "dest": "relative/path"})).get("error").is_some());
}

/// With `next_files` on, an edit of a file brings the files that earlier
/// sessions edited after it, each with its newest note; once per session.
#[test]
fn next_files_before_an_edit() {
    let dir = tempfile::tempdir().unwrap();
    let proj = tempfile::tempdir().unwrap();
    for f in ["src/ledger.rs", "tests/ledger.rs", "src/report.rs"] {
        let p = proj.path().join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }
    let p = |f: &str| proj.path().join(f).to_string_lossy().to_string();
    std::fs::write(dir.path().join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}, "next_files": true}"#).unwrap();
    let d = Arc::new(Daemon::open(dir.path(), &proj.path().to_string_lossy()).unwrap());
    let edit = |sid: &str, f: &str| hook(&d, "PostToolUse", json!({"session_id": sid, "tool_name": "Edit", "tool_input": {"file_path": p(f), "old_string": "a", "new_string": "b"}, "tool_response": {}}));

    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Add overdraft checks to the ledger"}));
    edit("s1", "src/ledger.rs");
    edit("s1", "src/ledger.rs");
    edit("s1", "tests/ledger.rs");
    call(&d, json!({"op": "remember", "kind": "fact", "text": "The ledger tests compare balances against the fixtures in tests/fixtures.", "files": ["tests/ledger.rs"]}));
    // A newer error about the test file: the fact is still the note shown.
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "cargo test --test ledger"},
        "tool_response": {"stdout": "error: test failed in tests/ledger.rs: overdraft_is_refused", "stderr": ""}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Added the overdraft check and its test."}));

    hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "Round balances to cents"}));
    let before = |f: &str| hook(&d, "PreToolUse", json!({"session_id": "s2", "tool_name": "Edit", "tool_input": {"file_path": p(f)}}));
    let text = before("src/ledger.rs");
    assert!(text.contains("edits of src/ledger.rs were usually followed by edits of: tests/ledger.rs."), "{text}");
    assert!(text.contains("ChloeCtx note on tests/ledger.rs") && text.contains("fixtures"), "{text}");
    assert_eq!(before("src/ledger.rs"), "");
    let log = std::fs::read_to_string(dir.path().join("events.log")).unwrap();
    assert_eq!(log.matches(r#""event":"next_files""#).count(), 1, "{log}");
    // Nothing was edited after the report module.
    assert_eq!(before("src/report.rs"), "");
}

/// A command that failed and then passed after an edit is written down as
/// a fix; the same error in a later session gets the fix with it, once.
#[test]
fn an_error_fixed_before_comes_back_with_its_fix() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path());
    let failing = json!({"stdout": "   Compiling ledger v0.1.0\nerror[E0425]: cannot find value `rate` in this scope\n --> /proj/src/ledger.rs:40:17", "exit_code": 101});
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Make the ledger tests pass"}));
    assert_eq!(hook(&d, "PostToolUseFailure", json!({"session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "cd /proj && cargo test 2>&1 | tail -30"}, "error": failing["stdout"]})), "");
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Edit",
        "tool_input": {"file_path": "/proj/src/ledger.rs", "old_string": "use crate::money;", "new_string": "use crate::money::{self, rate};"}, "tool_response": {}}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "cargo test 2>&1 | tail -30"},
        "tool_response": {"stdout": "test result: ok. 12 passed; 0 failed", "exit_code": 0}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Imported rate; the tests pass."}));
    let r = call(&d, json!({"op": "sql", "sql": "SELECT text, ctx, links FROM memory WHERE kind = 'error' AND tag = 'fix' LIMIT 5"}));
    let text = r["text"].as_str().unwrap();
    assert!(text.contains("Cleared by:") && text.contains("Edited src/ledger.rs") && text.contains("Signature: error[E#]: cannot find value `rate` in this scope"), "{r}");
    let links = call(&d, json!({"op": "sql", "sql": "SELECT links FROM memory WHERE kind = 'error' AND tag = 'fix' LIMIT 1"}));
    assert!(links["text"].as_str().unwrap().contains("from"), "the fix links to its turn's prompt: {links}");

    // A later session, the same error: the fix comes with it, once.
    hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "Run the tests again after the rebase"}));
    let fail = json!({"session_id": "s2", "tool_name": "Bash", "tool_input": {"command": "cargo test"}, "tool_response": {"stdout": "error[E0425]: cannot find value `rate` in this scope\n --> src/ledger.rs:52:9", "exit_code": 101}});
    let ctx = hook(&d, "PostToolUse", fail.clone());
    assert!(ctx.starts_with("ChloeCtx: this error was seen before"), "{ctx}");
    assert!(ctx.contains("use crate::money::{self, rate};") && ctx.contains("Then the same command passed."), "{ctx}");
    assert_eq!(hook(&d, "PostToolUse", fail), "", "shown once a session");
    // Another error is not matched to it.
    let other = hook(&d, "PostToolUse", json!({"session_id": "s2", "tool_name": "Bash", "tool_input": {"command": "cargo test"}, "tool_response": {"stdout": "error[E0308]: mismatched types", "exit_code": 101}}));
    assert_eq!(other, "");
}


/// CTX-7: an edit whose code quotes a harness tag (`<system-reminder>`) is
/// found by search and by the next session's recall, and is shown with the
/// tag defused, so it does not read as a notice from the harness.
#[test]
fn records_that_quote_a_harness_tag_are_recalled_defused() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path());
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Wrap the zorblax notice for the model in src/hooks.ts"}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Edit",
        "tool_input": {"file_path": "/proj/src/hooks.ts", "old_string": "// wrap",
            "new_string": "const zorblaxWrap = (s) => `<system-reminder>${s}</system-reminder>`;"}, "tool_response": {}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Added zorblaxWrap."}));

    hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    let s = call(&d, json!({"op": "search", "query": "zorblaxWrap hooks.ts", "limit": 10}));
    let text = s["text"].as_str().unwrap();
    assert!(text.contains("zorblaxWrap = (s)"), "the edit is not found: {text}");
    assert!(!text.contains("<system-reminder>") && text.contains("&lt;system-reminder>"), "{text}");
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": "how does zorblaxWrap wrap the zorblax notice in src/hooks.ts?"}));
    assert!(ctx.contains("zorblaxWrap = (s)"), "recall left out the edit: {ctx}");
    assert!(!ctx.contains("<system-reminder>"), "{ctx}");
}
