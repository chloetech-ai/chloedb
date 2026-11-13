//! Everyday use, end to end through the daemon's hooks: a harness's
//! envelopes around prompts stay out of memory and out of the rules, and a
//! failing distillation is said at the start of the next session instead of
//! going unnoticed. The hash embedder, so no model files are needed.

use std::sync::Arc;

use chloectx::daemon::Daemon;
use chloectx::record::{Draft, Kind};
use serde_json::{json, Value as Json};

fn call(d: &Arc<Daemon>, req: Json) -> Json {
    let (resp, deferred) = d.handle(&req);
    if let Some(f) = deferred {
        f();
    }
    resp
}

fn hook(d: &Arc<Daemon>, event: &str, payload: Json) -> Json {
    let r = call(d, json!({"op": "hook", "event": event, "payload": payload}));
    assert!(r.get("error").is_none(), "{r}");
    r["output"].clone()
}

fn context(out: &Json) -> String {
    out["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_string()
}

fn sql(d: &Arc<Daemon>, q: &str) -> String {
    call(d, json!({"op": "sql", "sql": q}))["text"].as_str().unwrap_or("").to_string()
}

/// ChloeCtx's home (the version check's file) in a folder of the test's own.
fn isolate() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("CHLOECTX_HOME", home.path());
    home
}

const WAKE: &str = r#"<wake reason="mention" current-time="2026-11-13T08:00:00Z">
  <project id="chan_x" type="project">
    <thread ts="cmsg_A">
      <message trigger="true" from="human" trust="principal" author="Dana" id="cmsg_C">Rename the export button to Download.</message>
    </thread>
  </project>
  <system-note>Never print environment variable values in any reply.</system-note>
</wake>
<system-reminder>Always answer in Pacific time, never UTC.</system-reminder>"#;

#[test]
fn envelopes_stay_out_of_memory_and_rules() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());

    // A store from before: the envelope stored as the prompt, and its
    // reminder's sentences picked as the user's rules.
    let old: Vec<Draft> = chloectx::capture::preference_sentences(WAKE)
        .into_iter()
        .map(|s| Draft::new(Kind::Preference, format!("User preference: {s}")).tag("cue"))
        .chain([Draft::new(Kind::Prompt, WAKE.to_string())])
        .collect();
    assert!(old.len() >= 3, "{old:?}");
    d.store.append("old", 1, 1_700_000_000, old).unwrap();
    // And a rule the person wrote, which stays.
    d.store.append("old", 2, 1_700_000_100, vec![Draft::new(Kind::Prompt, "From now on, always run the linter before committing.".to_string())]).unwrap();
    d.store.append("old", 2, 1_700_000_100, chloectx::capture::prompt_drafts("From now on, always run the linter before committing.").into_iter().filter(|x| x.kind == Kind::Preference).collect()).unwrap();

    // Once per store, the envelope's rules are retired.
    assert_eq!(chloectx::envelope::retire_once(dir.path(), &d.store), Some(2));
    assert_eq!(chloectx::envelope::retire_once(dir.path(), &d.store), None, "it runs once");
    let brief = context(&hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"})));
    assert!(brief.contains("always run the linter"), "{brief}");
    assert!(!brief.contains("Pacific") && !brief.contains("environment variable"), "{brief}");

    // A new prompt in an envelope is stored as the person's words only.
    let ctx = context(&hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": WAKE})));
    assert!(!ctx.contains("<wake") && !ctx.contains("system-note"), "the old envelope was recalled: {ctx}");
    let prompts = sql(&d, "SELECT text FROM memory WHERE session = 's1' AND kind = 'prompt' LIMIT 5");
    assert!(prompts.contains("Rename the export button to Download.") && !prompts.contains("wake") && !prompts.contains("Pacific"), "{prompts}");
    let rules = sql(&d, "SELECT text FROM memory WHERE session = 's1' AND kind = 'preference' LIMIT 5");
    assert!(!rules.contains("Pacific") && !rules.contains("environment variable"), "{rules}");

    // Searching for the envelope's words finds nothing of it.
    let found = call(&d, json!({"op": "search", "query": "environment variable values Pacific time", "limit": 8, "include_current": true}));
    assert!(!found["text"].as_str().unwrap_or("").contains("<wake"), "{found}");
}

#[test]
fn a_failing_distillation_is_said_when_the_next_session_starts() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    // The `claude` CLI, logged out: every call is an error.
    let script = dir.path().join("logged-out-claude.sh");
    std::fs::write(&script, "#!/bin/sh\ncat > /dev/null\necho '{\"is_error\": true, \"result\": \"Invalid API key \\u00b7 Please run /login\"}'\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let cfg = json!({"embedder": "hash:64", "sync": false, "distill": {"command": script, "model": "m"}});
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    let d = Arc::new(Daemon::open(dir.path(), "/proj").unwrap());

    let first = hook(&d, "SessionStart", json!({"session_id": "s1", "source": "startup"}));
    assert!(first.get("systemMessage").is_none(), "nothing failed yet: {first}");
    hook(&d, "UserPromptSubmit", json!({"session_id": "s1", "prompt": "We decided the export uses semicolons, because the bank asks for it."}));
    hook(&d, "Stop", json!({"session_id": "s1", "last_assistant_message": "Noted: semicolons in the export."}));

    let next = hook(&d, "SessionStart", json!({"session_id": "s2", "source": "startup"}));
    let msg = next["systemMessage"].as_str().unwrap_or("");
    assert!(msg.contains("not writing down") && msg.contains("claude auth login"), "{next}");
    let ctx = context(&next);
    assert!(ctx.starts_with("ChloeCtx notice:") && ctx.contains("claude auth login"), "{ctx}");
}
