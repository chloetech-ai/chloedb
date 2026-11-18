//! Whose words ChloeCtx keeps, and as what (persona study round 3, w1): a
//! private aside is kept nowhere, nor a later reply that says it again
//! (WEI-301, JONAH-301, WEI-304), a rule an agent
//! keeps is its suggestion until the user says it (LENA-310), how to do one
//! turn is not a rule (JONAH-303), a remembered copy replaces its twin and a
//! decision does not keep its status (JONAH-304), and a question is not
//! planned work nor a prompt's raw sentence a rule of the overview
//! (ANA-308). Uses the hash embedder and a fake model, so no model files
//! or API calls are needed.

use std::path::{Path, PathBuf};
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

fn ok(r: Json) -> Json {
    assert!(r.get("error").is_none(), "{r}");
    r
}

fn hook(d: &Arc<Daemon>, event: &str, payload: Json) -> String {
    let r = ok(call(d, json!({"op": "hook", "event": event, "payload": payload})));
    r["output"]["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string()
}

fn sql(d: &Arc<Daemon>, q: &str) -> Vec<Json> {
    let r = ok(call(d, json!({"op": "sql", "json": true, "sql": q})));
    r["rows"].as_array().cloned().unwrap()
}

/// Every file under `dir` whose bytes hold `s`.
fn files_holding(dir: &Path, s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_holding(&p, s));
        } else if std::fs::read(&p).is_ok_and(|b| b.windows(s.len()).any(|w| w == s.as_bytes())) {
            out.push(p.display().to_string());
        }
    }
    out
}

/// A model that answers with `out.json` and keeps what it was asked in
/// `asked`, outside the store's folder.
struct Model {
    out: PathBuf,
    asked: PathBuf,
    _dir: tempfile::TempDir,
}

impl Model {
    fn new() -> (Model, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.json");
        let asked = dir.path().join("asked");
        let script = dir.path().join("fake-model.sh");
        std::fs::write(&script, format!("#!/bin/sh\ncat >> '{}'\necho '<<END>>' >> '{}'\ncat '{}'\n", asked.display(), asked.display(), out.display())).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let m = Model { out, asked, _dir: dir };
        m.says(&[]);
        (m, script)
    }

    fn says(&self, lines: &[Json]) {
        let result: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(&self.out, json!({"is_error": false, "total_cost_usd": 0.001, "result": result.join("\n")}).to_string()).unwrap();
    }

    fn asked(&self) -> String {
        std::fs::read_to_string(&self.asked).unwrap_or_default()
    }

    fn calls(&self) -> usize {
        self.asked().matches("<<END>>").count()
    }
}

fn with_model(dir: &Path) -> (Arc<Daemon>, Model) {
    let (m, script) = Model::new();
    let cfg = json!({"embedder": "hash:64", "sync": false, "briefing_chars": 4000, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.join("config.json"), cfg.to_string()).unwrap();
    (Arc::new(Daemon::open(dir, "/proj").unwrap()), m)
}

fn every_text(d: &Arc<Daemon>) -> String {
    sql(d, "SELECT kind, tag, text, ctx FROM memory LIMIT 1000").iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n")
}

const WEI: &str = "Off the record, please don't save this anywhere: I'm interviewing at another company next week, so I may be slow on reviews on Thursday. Anyway, for the deploy work: staging uses DATABASE_URL=postgres://ledger_app:hunter2-fake@db.staging.acme.internal:5432/ledger. Add a .env.example with placeholders for these and a one-paragraph note in the README on where each value comes from.";

/// WEI-301: an "off the record, don't save this" aside was stored word for
/// word, made a rule every later session was told, and synced to the team;
/// Claude told the user it was not saved.
#[test]
fn an_off_the_record_aside_is_kept_nowhere() {
    let store = tempfile::tempdir().unwrap();
    let (d, m) = with_model(store.path());
    // What a model given the aside would write down: it is not given it,
    // and what says it all the same is not kept.
    m.says(&[
        json!({"kind": "rule", "text": "Off the record: the user is interviewing at another company next week and may be slow on reviews on Thursday.", "said_by": "requester"}),
        json!({"kind": "decision", "text": "Staging's settings are documented in .env.example with placeholders."}),
    ]);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": WEI}));
    assert!(ctx.contains("private aside the user asked not to keep") && ctx.contains("left it out"), "the session is told: {ctx}");
    assert!(!ctx.contains("interviewing"), "{ctx}");
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Write", "tool_input": {"file_path": "/proj/.env.example", "content": "DATABASE_URL=postgres://user:password@host:5432/db\n"}, "tool_response": {"type": "create"}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Good luck with the interview! I added .env.example with placeholders for DATABASE_URL and a note in the README on where each value comes from."}));

    // The model was asked about the work, not the aside.
    assert_eq!(m.calls(), 1, "the turn is distilled");
    let asked = m.asked();
    assert!(asked.contains(".env.example") && !asked.contains("interviewing") && !asked.contains("interview!"), "{asked}");
    // No record holds it: not the prompt, not its context, not a rule.
    let all = every_text(&d);
    assert!(!all.contains("interviewing") && !all.contains("Good luck with the interview"), "{all}");
    let prompt = sql(&d, "SELECT text FROM memory WHERE kind = 'prompt' LIMIT 5");
    assert!(prompt[0][0].as_str().unwrap().contains("[private aside left out]"), "{prompt:?}");
    assert!(prompt[0][0].as_str().unwrap().contains(".env.example"), "the work is kept: {prompt:?}");
    let rules = sql(&d, "SELECT text FROM memory WHERE kind = 'preference' LIMIT 5");
    assert!(rules.is_empty(), "no rule: {rules:?}");
    // Nor any file of the store: state, events, receipts, the database.
    let left = files_holding(store.path(), "interviewing");
    assert!(left.is_empty(), "the aside is in {left:?}");

    // A later turn's reply that says it again loses that sentence too
    // (WEI-304: "I didn't save your interview note" survived a forget).
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Did you save any of that to memory?"}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "No. I didn't save your interview note or any of the credentials to memory. The .env.example only holds placeholders."}));
    let all = every_text(&d);
    assert!(!all.contains("interview note") && all.contains("only holds placeholders"), "{all}");

    // A later session is not told it, nor finds it.
    let brief = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    assert!(!brief.contains("interview") && brief.contains("never tell the user that something they wrote was not saved"), "{brief}");
    let found = call(&d, json!({"op": "search", "query": "interviewing at another company", "limit": 10, "include_current": true}));
    assert!(!found["text"].as_str().unwrap_or("").contains("interviewing"), "{found}");
}

/// JONAH-301: a parenthetical "keep that name private" was stored and
/// recalled; and a prompt that is private as a whole keeps nothing.
#[test]
fn a_private_name_and_a_private_turn_are_kept_nowhere() {
    let store = tempfile::tempdir().unwrap();
    let (d, m) = with_model(store.path());
    m.says(&[json!({"kind": "fact", "text": "The client this is built for is Bramble Bakery."})]);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    let prompt = "Add a --version flag that prints the version from pyproject. (Side note, unrelated: the client I'm eventually building this for is Bramble Bakery; keep that name private, it's not public yet.) Commit when the tests pass.";
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": prompt}));
    assert!(ctx.contains("left it out"), "{ctx}");
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Edit", "tool_input": {"file_path": "/proj/cli.py", "old_string": "x", "new_string": "y"}, "tool_response": {}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Added --version; it reads pyproject.toml. The client is Bramble Bakery, noted. All 12 tests pass."}));
    assert!(!m.asked().contains("Bramble"), "{}", m.asked());
    // The fake model wrote it as a fact all the same: not kept either.
    let holding: Vec<Json> = sql(&d, "SELECT kind, tag, text, ctx FROM memory LIMIT 1000").into_iter().filter(|r| r.to_string().contains("Bramble")).collect();
    assert!(holding.is_empty(), "{holding:?}");
    let left = files_holding(store.path(), "Bramble");
    assert!(left.is_empty(), "the name is in {left:?}");
    let answer = sql(&d, "SELECT text FROM memory WHERE kind = 'answer' LIMIT 5");
    assert!(answer.iter().all(|a| !a.to_string().contains("Bramble")) && answer.iter().any(|a| a.to_string().contains("All 12 tests pass")), "{answer:?}");

    // A turn that is private as a whole: nothing of it is kept or distilled.
    let before = sql(&d, "SELECT id FROM memory WHERE session = 's1' LIMIT 1000").len();
    let calls = m.calls();
    let ctx = hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Off the record, don't save any of this: I think the CTO is wrong about moving the queue to Kafka."}));
    assert!(ctx.contains("keeps nothing of this turn"), "{ctx}");
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Read", "tool_input": {"file_path": "/proj/queue.py"}, "tool_response": {}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Understood. Moving the queue to Kafka has trade-offs worth raising with the CTO."}));
    assert_eq!(m.calls(), calls, "a private turn is not distilled");
    let kept: Vec<Json> = sql(&d, "SELECT kind, tag, text FROM memory WHERE session = 's1' LIMIT 1000");
    let new: Vec<&Json> = kept.iter().filter(|r| r.to_string().contains("Kafka") || r.to_string().contains("CTO")).collect();
    assert!(new.is_empty(), "{new:?}");
    assert!(kept.len() <= before + 1, "at most a mark that the turn was skipped: {before} then {kept:?}");
    let left = files_holding(store.path(), "Kafka");
    assert!(left.is_empty(), "the private turn is in {left:?}");
    // The next turn is kept as usual.
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Now add a --help example to the README."}));
    let all = every_text(&d);
    assert!(all.contains("--help example"), "{all}");
}

/// LENA-310: a preference the agent kept with `said_by: assistant` was
/// listed under "The user's rules"; one it says the user said, in words the
/// user never wrote, was too.
#[test]
fn a_rule_the_agent_keeps_is_its_suggestion() {
    let store = tempfile::tempdir().unwrap();
    std::fs::write(store.path().join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    let d = Arc::new(Daemon::open(store.path(), "/proj").unwrap());
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Add retries to the payment client. From now on, always run the integration tests before you push."}));
    // The agent's own idea, and one it says is the user's: suggestions.
    let a = ok(call(&d, json!({"op": "remember", "kind": "preference", "text": "Prefer exponential backoff capped at 30 seconds for every HTTP client.", "said_by": "assistant"})));
    assert_eq!(a["suggestion"], json!(true), "{a}");
    let b = ok(call(&d, json!({"op": "remember", "kind": "preference", "text": "Never mock the payment gateway in unit tests.", "said_by": "user"})));
    assert_eq!(b["suggestion"], json!(true), "the user never said it: {b}");
    assert!(b["text"].as_str().unwrap().contains("the user's messages in this session do not say it"), "{b}");
    // What the user did say is the user's rule, as the CLI's note is.
    let c = ok(call(&d, json!({"op": "remember", "kind": "preference", "text": "Always run the integration tests before you push.", "said_by": "user"})));
    assert_eq!(c["suggestion"], json!(false), "{c}");
    let e = ok(call(&d, json!({"op": "remember", "kind": "preference", "text": "Use the staging gateway for manual tests.", "by": "user"})));
    assert_eq!(e["suggestion"], json!(false), "{e}");

    let brief = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    let rules = brief.split("The user's rules").nth(1).unwrap_or("");
    let (rules, suggested) = rules.split_once("Suggested by an agent").unwrap_or((rules, ""));
    assert!(rules.contains("integration tests before you push") && rules.contains("staging gateway"), "{brief}");
    assert!(!rules.contains("exponential backoff") && !rules.contains("mock the payment gateway"), "{brief}");
    assert!(suggested.contains("exponential backoff") && suggested.contains("mock the payment gateway"), "{brief}");
    // The command center lists them as notes, not rules.
    let o = chloectx::app::view::overview(&d.store).unwrap();
    let rules = o["rules"].to_string();
    assert!(rules.contains("integration tests") && !rules.contains("exponential backoff"), "{o}");
    assert!(o["notes"].to_string().contains("exponential backoff"), "{o}");
}

/// JONAH-303: "don't write tests and don't commit yet" was kept as a rule,
/// and "no code changes" said in two sessions became a standing one.
#[test]
fn how_to_do_one_turn_is_not_a_rule() {
    let store = tempfile::tempdir().unwrap();
    let (d, m) = with_model(store.path());
    m.says(&[
        json!({"kind": "rule", "text": "Don't write tests and don't commit yet.", "said_by": "requester"}),
        json!({"kind": "rule", "text": "Use ruff for linting.", "said_by": "requester"}),
    ]);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Ground rules for this repo: always use ruff for linting. Sketch the export command; don't write tests and don't commit yet."}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Write", "tool_input": {"file_path": "/proj/export.py", "content": "def export(): ..."}, "tool_response": {"type": "create"}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Sketched export.py; no tests and nothing committed yet."}));
    let rules: Vec<String> = sql(&d, "SELECT text, tag FROM memory WHERE kind = 'preference' LIMIT 20").iter().map(|r| r.to_string()).collect();
    assert!(rules.iter().any(|r| r.contains("ruff")), "the ground rule is kept: {rules:?}");
    assert!(rules.iter().all(|r| !r.contains("commit yet") && !r.contains("write tests")), "{rules:?}");

    // Said again in a second session: still how to do that turn.
    m.says(&[json!({"kind": "rule", "text": "No code changes: just answer.", "said_by": "requester"})]);
    for sid in ["s2", "s3"] {
        hook(&d, "SessionStart", json!({"session_id": sid, "source": "startup"}));
        hook(&d, "UserPromptSubmit", json!({"session_id": sid, "prompt": "No code changes, just answer: why does the export command buffer every row before writing?"}));
        hook(&d, "Stop", json!({"session_id": sid, "last_assistant_message": "It buffers rows to sort them by date before writing. ".repeat(6)}));
    }
    let rules: Vec<String> = sql(&d, "SELECT text, tag FROM memory WHERE kind = 'preference' LIMIT 20").iter().map(|r| r.to_string()).collect();
    assert!(rules.iter().all(|r| !r.to_lowercase().contains("no code changes")), "{rules:?}");
    let brief = hook(&d, "SessionStart", json!({"session_id": "s4", "source": "startup"}));
    let users = brief.split("The user's rules").nth(1).unwrap_or("");
    assert!(!users.to_lowercase().contains("no code changes") && !users.contains("commit yet"), "{brief}");
}

/// JONAH-304: the decision kept as "Not implemented yet; planned for a
/// later session" stayed so once it was built, and the prompt's copy of it
/// and the one remembered were both listed.
#[test]
fn a_remembered_decision_replaces_its_twin_and_drops_its_status() {
    let store = tempfile::tempdir().unwrap();
    std::fs::write(store.path().join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    let d = Arc::new(Daemon::open(store.path(), "/proj").unwrap());
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Decision: the --json flag prints one object per line, so the output can be piped into jq."}));
    let stated = sql(&d, "SELECT id FROM memory WHERE kind = 'decision' AND tag = 'stated' LIMIT 5");
    assert_eq!(stated.len(), 1, "{stated:?}");
    let r = ok(call(&d, json!({"op": "remember", "kind": "decision", "text": "The --json flag prints one object per line so the output can be piped into jq. Not implemented yet; planned for a later session.", "said_by": "user"})));
    assert_eq!(r["replaces"], json!([stated[0][0]]), "{r}");
    let live = sql(&d, "SELECT text, tag FROM memory WHERE kind = 'decision' AND tag != 'superseded' LIMIT 5");
    assert_eq!(live.len(), 1, "one decision is one note: {live:?}");
    let text = live[0][0].as_str().unwrap();
    assert!(text.contains("piped into jq") && !text.contains("Not implemented") && !text.contains("later session"), "{text}");
    // Asked, it is one note (the excerpt after the notes is the prompt).
    let asked = ok(call(&d, json!({"op": "ask", "question": "How does the --json flag print its output?", "include_current": true})));
    let text = asked["text"].as_str().unwrap();
    let notes = text.split("Excerpts of the messages").next().unwrap();
    assert_eq!(notes.matches("decision:").count(), 1, "{text}");
}

/// ANA-308: "How should I add it?" was filed as planned work, and the
/// overview showed a prompt's raw sentences as the project's rules.
#[test]
fn a_question_is_not_planned_work_nor_a_cue_a_rule() {
    let store = tempfile::tempdir().unwrap();
    let (d, m) = with_model(store.path());
    m.says(&[json!({"kind": "requirement", "text": "Add rate limiting to the public API."})]);
    hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "We may want rate limiting on the public API one day. How should I add it?"}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "A token bucket per API key in the gateway would do it; the middleware sits before the routes. ".repeat(4)}));
    assert_eq!(m.calls(), 1);
    let planned = sql(&d, "SELECT text FROM memory WHERE kind = 'todo' LIMIT 5");
    assert!(planned.is_empty(), "a question is not planned work: {planned:?}");

    // Asked for, it is.
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "Next sprint we add rate limiting to the public API; plan it as a task."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Noted for next sprint."}));
    assert_eq!(sql(&d, "SELECT text FROM memory WHERE kind = 'todo' LIMIT 5").len(), 1);

    // A rule the prompt states is a cue; once a rule is distilled, the
    // overview lists the distilled rules, not the raw sentences.
    m.says(&[json!({"kind": "rule", "text": "Always add a changelog entry with each pull request.", "said_by": "requester"})]);
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "From now on, always add a changelog entry with each pull request, ok? Please update the README too."}));
    hook(&d, "PostToolUse", json!({"session_id": "s1", "tool_name": "Edit", "tool_input": {"file_path": "/proj/README.md", "old_string": "a", "new_string": "b"}, "tool_response": {}}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Updated the README; I will add a changelog entry with each pull request."}));
    let o = chloectx::app::view::overview(&d.store).unwrap();
    let rules = o["rules"].as_array().unwrap();
    assert!(!rules.is_empty() && rules.iter().all(|r| r["tag"] != "cue"), "{o}");
    assert!(!o["rules"].to_string().contains("ok?") && !o["rules"].to_string().contains("README"), "{o}");
}

/// A store written before asides were cut keeps the rules picked from them:
/// they are retired once (the daemon does it when it starts), and the rest
/// stay.
#[test]
fn rules_picked_from_an_old_aside_are_retired() {
    use chloectx::record::{Draft, Kind};
    let store = tempfile::tempdir().unwrap();
    std::fs::write(store.path().join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    let d = Arc::new(Daemon::open(store.path(), "/proj").unwrap());
    let old = Draft::new(Kind::Preference, "User preference: Off the record, please don't save this anywhere: I'm interviewing at another company next week").tag("cue");
    let keep = Draft::new(Kind::Preference, "User preference: always run clippy before committing").tag("cue");
    d.store.append("old", 1, 1_700_000_000_000, vec![old, keep]).unwrap();
    assert_eq!(chloectx::private::retire_once(store.path(), &d.store), Some(1));
    assert_eq!(chloectx::private::retire_once(store.path(), &d.store), None, "once");
    let brief = hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    assert!(brief.contains("clippy") && !brief.contains("interviewing"), "{brief}");
}
