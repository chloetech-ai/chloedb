//! Forgetting a private fact is safe and complete (the persona study's
//! JONAH-1, JONAH-2 and LENA-5): the rules said in the same prompt stay,
//! no file of the store holds the fact afterwards, and the turn that asked
//! for it to be forgotten writes no record that says it again. Its own
//! process: it sets CHLOECTX_HOME. Uses the hash embedder and a fake
//! distiller, so no model is called.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chloectx::daemon::Daemon;
use serde_json::{json, Value as Json};

const FACT: &str = "Quokka-Bundle-77";

fn call(d: &Arc<Daemon>, req: Json) -> Json {
    let (resp, deferred) = d.handle(&req);
    if let Some(f) = deferred {
        f();
    }
    assert!(resp.get("error").is_none(), "{req} -> {resp}");
    resp
}

fn hook(d: &Arc<Daemon>, event: &str, payload: Json) -> String {
    let r = call(d, json!({"op": "hook", "event": event, "payload": payload}));
    r["output"]["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string()
}

fn rows(d: &Arc<Daemon>, sql: &str) -> Vec<Json> {
    call(d, json!({"op": "sql", "json": true, "sql": sql}))["rows"].as_array().cloned().unwrap_or_default()
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            v.extend(walk(&p));
        } else {
            v.push(p);
        }
    }
    v
}

/// Every file under the store that holds `s`, whatever its case, with how
/// many times.
fn files_holding(dir: &Path, s: &str) -> Vec<String> {
    let n = s.to_ascii_lowercase();
    walk(dir)
        .into_iter()
        .filter_map(|p| {
            let b = std::fs::read(&p).ok()?.to_ascii_lowercase();
            let c = b.windows(n.len()).filter(|w| *w == n.as_bytes()).count();
            (c > 0).then(|| format!("{} x{c}", p.strip_prefix(dir).unwrap().display()))
        })
        .collect()
}

struct Model {
    out: PathBuf,
    _dir: tempfile::TempDir,
    script: PathBuf,
}

impl Model {
    /// A distiller that answers what it was last told to, kept outside
    /// the store so that its files are not the store's.
    fn new() -> Model {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.json");
        let script = dir.path().join("fake-model.sh");
        std::fs::write(&script, format!("#!/bin/sh\ncat > /dev/null\ncat '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let m = Model { out, _dir: dir, script };
        m.says(&[]);
        m
    }

    fn says(&self, items: &[Json]) {
        let result: Vec<String> = items.iter().map(|l| l.to_string()).collect();
        std::fs::write(&self.out, json!({"is_error": false, "total_cost_usd": 0.001, "result": result.join("\n")}).to_string()).unwrap();
    }
}

fn rule(text: &str) -> Json {
    json!({"kind": "rule", "text": text, "said_by": "requester", "scope": "standing"})
}

const RULES: [&str; 3] = [
    "Write all tests with pytest and never use unittest.",
    "tdo stores todos in SQLite, never in a JSON file, because a JSON file got corrupted when two terminals wrote to it at once.",
    "Type hints are required everywhere in tdo.",
];

#[test]
fn a_private_fact_is_forgotten_everywhere_and_the_rules_said_with_it_stay() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("CHLOECTX_HOME", home.path());
    let model = Model::new();
    let store = tempfile::tempdir().unwrap();
    let dir = store.path();
    let cfg = json!({"embedder": "hash:64", "sync": false, "briefing_chars": 4000,
        "receipts": {"enabled": true, "snapshots": {"min_interval_secs": 0}},
        "distill": {"command": model.script, "model": "m"}});
    std::fs::write(dir.join("config.json"), cfg.to_string()).unwrap();
    let d = Arc::new(Daemon::open(dir, "/proj").unwrap());

    // The first prompt says the private plan and the ground rules at once
    // (Jonah's first session): the rules distilled from it carry the plan
    // in their context.
    model.says(&[rule(RULES[0]), rule(RULES[1]), rule(RULES[2]), json!({"kind": "fact", "text": format!("tdo will be sold later as part of the {FACT}."), "said_by": "requester"})]);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": format!("I'm building tdo, a tiny todo app in Python that I'll sell as part of the {FACT} later. Ground rules: pytest never unittest, SQLite not JSON, type hints everywhere.")}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Bash", "tool_use_id": "t1", "tool_input": {"command": "cat NOTES.md"}, "tool_response": {"stdout": format!("bundle: {FACT}\nprice: 9 USD"), "stderr": ""}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Noted the ground rules."}));
    model.says(&[]);
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": format!("What is the plan for the {}?", FACT.to_lowercase())}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": format!("You plan to sell tdo as part of the {FACT}.")}));
    let shown = call(&d, json!({"op": "search", "query": format!("{FACT} plan"), "include_current": true}));
    assert!(!shown["ids"].as_array().unwrap().is_empty(), "{shown}");
    // What an earlier version logged of a search, and an error message.
    let mut log = std::fs::OpenOptions::new().append(true).open(dir.join("events.log")).unwrap();
    std::io::Write::write_all(&mut log, format!("{{\"event\":\"deep\",\"query\":\"text:selling tdo in the {FACT}\"}}\n").as_bytes()).unwrap();
    drop(log);
    std::fs::write(dir.join("daemon.log"), format!("chloectx: started\nchloectx: distill failed on \"sell it in the {FACT}\"\n")).unwrap();
    d.persist();
    let rules_before = rows(&d, "SELECT id, text, ctx FROM memory WHERE kind = 'preference' AND tag != 'superseded' LIMIT 20");
    let ours: Vec<&Json> = rules_before.iter().filter(|r| RULES.contains(&r[1].as_str().unwrap())).collect();
    assert_eq!(ours.len(), 3, "{rules_before:?}");
    assert!(ours.iter().all(|r| r[2].as_str().unwrap().contains(FACT)), "the rules carry it in their context: {ours:?}");
    let held = files_holding(dir, FACT);
    assert!(held.iter().any(|f| f.starts_with("state.json")) && held.iter().any(|f| f.starts_with("receipts.strata")), "{held:?}");
    assert!(std::fs::read_dir(dir.join("snapshots")).is_ok_and(|mut r| r.next().is_some()), "an automatic snapshot was kept");

    // JONAH-1: what a forget of it would delete is what holds it in its
    // own text; the rules are listed as kept.
    let f = call(&d, json!({"op": "find", "text": FACT.to_uppercase()}));
    let kinds: Vec<&str> = f["matches"].as_array().unwrap().iter().map(|m| m["kind"].as_str().unwrap()).collect();
    for k in ["prompt", "tool_result", "fact", "answer"] {
        assert!(kinds.contains(&k), "{k} in {f}");
    }
    assert!(!kinds.contains(&"preference"), "{f}");
    let kept: Vec<&str> = f["context"].as_array().unwrap().iter().map(|m| m["line"].as_str().unwrap()).collect();
    for r in RULES {
        assert!(kept.iter().any(|l| r.starts_with(l.trim_end_matches('…'))), "{r} is kept: {f}");
    }
    assert!(f["matches"].as_array().unwrap().iter().all(|m| !m["line"].as_str().unwrap().is_empty()), "each with its first line: {f}");
    assert!(f["side"]["sessions"].as_u64() >= Some(1) && f["side"]["receipts"].as_u64() >= Some(1) && f["side"]["log_lines"].as_u64() >= Some(2), "{f}");

    // JONAH-2: the agent is asked to forget it, searches, and forgets it
    // by its text; the distiller would note that the plan is private.
    model.says(&[json!({"kind": "fact", "text": format!("The user's plan to sell tdo in the {FACT} is private and must not be stored."), "said_by": "requester"}), rule("When the user asks to forget their private plan, delete those records.")]);
    hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": format!("Find what memory holds about my {FACT} plan and forget it: that plan is private.")}));
    let found = call(&d, json!({"op": "search", "query": FACT}));
    hook(&d, "PostToolUse", json!({"session_id": "s2", "tool_name": "mcp__chloectx__memory_search", "tool_use_id": "t2", "tool_input": {"query": FACT}, "tool_response": [{"type": "text", "text": found["text"]}]}));
    let gone = call(&d, json!({"op": "forget", "text": FACT}));
    assert!(gone["deleted"].as_array().unwrap().len() >= 6, "{gone}");
    assert!(gone["cut"].as_array().unwrap().len() >= 3, "{gone}");
    assert!(gone["text"].as_str().unwrap().contains("which stay"), "{gone}");
    assert!(!dir.join("snapshots").read_dir().is_ok_and(|r| r.flatten().any(|e| e.path().is_dir())), "the automatic snapshots were dropped");
    hook(&d, "PostToolUse", json!({"session_id": "s2", "tool_name": "mcp__chloectx__memory_forget", "tool_use_id": "t3", "tool_input": {"text": FACT}, "tool_response": [{"type": "text", "text": gone["text"]}]}));
    // Late: a search's output that arrives after the forget.
    hook(&d, "PostToolUse", json!({"session_id": "s2", "tool_name": "Bash", "tool_use_id": "t4", "tool_input": {"command": "grep -ri bundle ."}, "tool_response": {"stdout": format!("NOTES.md:bundle: {FACT}"), "stderr": ""}}));
    let n_before = rows(&d, "SELECT COUNT(*) FROM memory LIMIT 1");
    hook(&d, "Stop", json!({"session_id": "s2", "last_assistant_message": format!("Done: I deleted every record about your {FACT} plan, and nothing about it is kept.")}));
    assert_eq!(rows(&d, "SELECT COUNT(*) FROM memory LIMIT 1"), n_before, "the forget turn stores no reply, card or note");
    // A paraphrase is not stored either: the turn is not distilled.
    assert!(rows(&d, "SELECT id, text FROM memory WHERE session = 's2' AND kind IN ('fact', 'preference', 'decision', 'note', 'todo', 'answer', 'episode') LIMIT 10").is_empty());
    // Said again in the next few minutes, it is not stored.
    let refused = d.handle(&json!({"op": "remember", "kind": "fact", "text": format!("Remember the {FACT}.")})).0;
    assert!(refused["error"].as_str().is_some_and(|e| e.contains("forgotten")), "{refused}");
    hook(&d, "UserPromptSubmit", json!({"session_id": "s2", "prompt": format!("Did you forget the {FACT}?")}));
    model.says(&[json!({"kind": "fact", "text": format!("The user checked that the {FACT} was forgotten."), "said_by": "requester"})]);
    hook(&d, "Stop", json!({"session_id": "s2", "last_assistant_message": "Yes."}));
    d.persist();

    // (a) Every rule stays, its context cut, and the briefing still has it.
    let rules_after = rows(&d, "SELECT id, text, ctx FROM memory WHERE kind = 'preference' AND tag != 'superseded' LIMIT 20");
    for r in RULES {
        let kept = rules_after.iter().find(|x| x[1] == r).unwrap_or_else(|| panic!("{r} was deleted: {rules_after:?}"));
        let ctx = kept[2].as_str().unwrap();
        assert!(!ctx.to_lowercase().contains(&FACT.to_lowercase()) && ctx.contains("By: user"), "{ctx}");
    }
    let brief = hook(&d, "SessionStart", json!({"session_id": "s3", "source": "startup"}));
    for r in RULES {
        assert!(brief.contains(r.trim_end_matches('.')), "{r} in {brief}");
    }
    // (c) No record says it again, in any words of it.
    assert!(rows(&d, &format!("SELECT id FROM memory WHERE MATCH(body, 'quokka') LIMIT 10")).is_empty());
    let all = rows(&d, "SELECT text, ctx FROM memory LIMIT 1000");
    assert!(all.iter().all(|r| !r.to_string().to_lowercase().contains("quokka")), "{all:?}");
    assert!(all.iter().all(|r| !r[0].as_str().unwrap().contains("is private")), "{all:?}");
    // (b) No file of the store holds it.
    assert_eq!(files_holding(dir, FACT), Vec::<String>::new());
    let log = std::fs::read_to_string(dir.join("daemon.log")).unwrap();
    assert!(log.starts_with("chloectx: started\n") && log.lines().count() == 2, "{log}");
}
