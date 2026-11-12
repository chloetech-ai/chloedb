//! Secrets in a store: what the hooks capture is redacted before it is
//! stored, and `redact` rewrites a store written before redaction knew a
//! secret's shape (its records, session state and event log, and the
//! sync's server's copies). Its own process: it sets CHLOECTX_HOME and the
//! sync's URL. Uses the hash embedder, so no model files are needed.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use chloectx::daemon::Daemon;
use chloectx::record::{Kind, Record};
use serde_json::{json, Value as Json};

// Fake secrets, built at run time so that no literal in this file has the
// shape of a real one.
fn github_token() -> String {
    format!("{}_{}", "ghp", "Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0FeDcBa98")
}
fn db_password() -> String {
    ["correct", "horse", "battery", "42"].join("-")
}
fn anthropic_key() -> String {
    format!("{}-{}-{}", "sk", "ant", "api03-Qw3rTy7uIoP9aSdF2gHjK4lZxC6vBnM8")
}

fn call(d: &Arc<Daemon>, req: Json) -> Json {
    let (resp, deferred) = d.handle(&req);
    if let Some(f) = deferred {
        f();
    }
    assert!(resp.get("error").is_none(), "{resp}");
    resp
}

fn daemon(dir: &std::path::Path) -> Arc<Daemon> {
    std::fs::write(dir.join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    Arc::new(Daemon::open(dir, "/proj").unwrap())
}

/// Every record's text and context.
fn all_text(d: &Daemon) -> String {
    let rs = d.store.query("SELECT text, ctx FROM memory LIMIT 100000").unwrap();
    rs.rows.iter().flat_map(|r| r.iter().filter_map(|v| v.as_str().map(String::from))).collect::<Vec<_>>().join("\n")
}

/// Whether the store's database file or its write-ahead log holds `s`.
fn file_holds(dir: &std::path::Path, s: &str) -> bool {
    ["memory.strata", "memory.strata-wal"]
        .iter()
        .filter_map(|n| std::fs::read(dir.join(n)).ok())
        .any(|bytes| bytes.windows(s.len()).any(|w| w == s.as_bytes()))
}

/// A server that answers every POST with `{"written": 1}` and keeps the bodies.
fn fake_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let b = bodies.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let (mut len, mut expect) = (0usize, false);
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let h = line.trim_end().to_ascii_lowercase();
                if h.is_empty() {
                    break;
                }
                if let Some(v) = h.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
                expect |= h.starts_with("expect:");
            }
            if expect {
                let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
            let mut body = vec![0; len];
            if r.read_exact(&mut body).is_err() {
                continue;
            }
            b.lock().unwrap().push(String::from_utf8_lossy(&body).into_owned());
            let resp = r#"{"written": 1}"#;
            let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}", resp.len());
        }
    });
    (url, bodies)
}

#[test]
fn secrets_are_redacted_as_captured_and_redact_rewrites_an_old_store() {
    let home = tempfile::tempdir().unwrap();
    let (url, bodies) = fake_server();
    std::env::set_var("CHLOECTX_HOME", home.path());
    std::env::set_var("CHLOECTX_SYNC_URL", &url);
    std::env::remove_var("CHLOECTX_SYNC_TOKEN");
    for k in ["NO_PROXY", "no_proxy"] {
        std::env::set_var(k, "127.0.0.1,localhost");
    }
    let (gh, pw, ant) = (github_token(), db_password(), anthropic_key());

    // A store as an older version left it: records, session state and
    // event log that hold secrets.
    let dir = tempfile::tempdir().unwrap();
    let state = json!({"active": "s0", "sessions": {"s0": {"window_start": 0, "turn": 1, "injected": [], "annotated": [],
        "episode": {"prompt": format!("deploy with GITHUB_TOKEN={gh}"), "files": [], "commands": [], "errors": [], "tools": 0},
        "episode_stored": true, "last_prompts": [format!("deploy with GITHUB_TOKEN={gh}")], "last_compact": 0, "last_seen": 1, "transcript_offset": 0}}});
    std::fs::write(dir.path().join("state.json"), state.to_string()).unwrap();
    std::fs::write(dir.path().join("events.log"), format!("{}\n{}\n", json!({"event": "ask", "question": format!("why does DB_PASSWORD={pw} fail")}), json!({"event": "x"}))).unwrap();
    let d = daemon(dir.path());
    let old = |id: &str, seq: i64, text: String, ctx: String| Record {
        id: id.into(),
        session: "s0".into(),
        kind: Kind::ToolResult,
        tag: "Bash".into(),
        turn: 1,
        seq,
        ts: 1_700_000_000,
        importance: 0.3,
        files: String::new(),
        text,
        ctx,
        links: Vec::new(),
    };
    let recs = vec![
        old("s0:1", 1, format!("$ env\nHOME=/home/me\nDB_PASSWORD={pw}\nANTHROPIC_API_KEY={ant}"), "Turn 1 request: print the environment".into()),
        old("s0:2", 2, "$ git push\nEverything up-to-date".into(), format!("Turn 1 request: push with {gh}")),
        old("s0:3", 3, "cargo test: 12 passed".into(), "Turn 1 request: run the tests".into()),
    ];
    let texts: Vec<String> = recs.iter().map(|r| r.embed_text()).collect();
    let embs = d.store.embedder.embed(&texts.iter().map(|s| s.as_str()).collect::<Vec<_>>()).unwrap();
    d.store.bulk_insert(&recs, embs).unwrap();
    let before = all_text(&d);
    assert!(before.contains(&pw) && before.contains(&gh) && before.contains(&ant));
    d.store.flush().unwrap();
    assert!(file_holds(dir.path(), &pw), "the store's file should hold the text as written");

    // A dry run counts and changes nothing.
    let r = call(&d, json!({"op": "redact", "dry_run": true}));
    assert_eq!((r["records"].as_u64(), r["changed"].as_u64(), r["sessions"].as_u64(), r["log_lines"].as_u64()), (Some(3), Some(2), Some(1), Some(1)), "{r}");
    assert_eq!(all_text(&d), before);
    assert!(bodies.lock().unwrap().is_empty());

    let r = call(&d, json!({"op": "redact"}));
    let shown = r.to_string();
    for s in [&pw, &gh, &ant] {
        assert!(!shown.contains(s.as_str()), "the report shows a secret: {shown}");
    }
    assert_eq!(r["changed"], json!(2), "{r}");
    assert_eq!(r["kinds"]["password"], json!(2), "{r}");
    assert_eq!(r["kinds"]["anthropic-key"], json!(1), "{r}");
    assert!(r["kinds"]["github-token"].as_u64().unwrap() >= 3, "{r}");
    assert_eq!(r["compacted"], json!(true), "{r}");
    assert_eq!(r["sync"]["replaced"], json!(2), "{r}");
    assert!(r["text"].as_str().unwrap().starts_with("2 of 3 records redacted (anthropic-key x1, github-token x"), "{r}");

    let after = all_text(&d);
    for s in [&pw, &gh, &ant] {
        assert!(!after.contains(s.as_str()), "{after}");
    }
    assert!(after.contains("DB_PASSWORD=[REDACTED:password]") && after.contains("HOME=/home/me"), "{after}");
    assert!(after.contains("push with [REDACTED:github-token]"), "{after}");
    assert_eq!(d.store.len(), 3);
    // Still found by what is not secret.
    let s = call(&d, json!({"op": "search", "query": "environment DB_PASSWORD", "limit": 3, "include_current": true}));
    assert!(s["text"].as_str().unwrap().contains("REDACTED:password"), "{s}");
    // The session state and the log.
    let state = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
    assert!(!state.contains(&gh) && state.contains("GITHUB_TOKEN=[REDACTED:github-token]"), "{state}");
    let log = std::fs::read_to_string(dir.path().join("events.log")).unwrap();
    assert!(!log.contains(&pw) && log.contains("DB_PASSWORD=[REDACTED:password]") && log.contains(r#""event":"x""#), "{log}");
    // The server got the records again, redacted.
    let sent = bodies.lock().unwrap().join("\n");
    assert!(sent.contains("s0:1") && sent.contains("s0:2") && !sent.contains("s0:3"), "{sent}");
    for s in [&pw, &gh, &ant] {
        assert!(!sent.contains(s.as_str()), "{sent}");
    }
    // The old versions are gone from the file, not only from queries.
    // The pages of the old versions are free but still in the file, until
    // it is rewritten (which `chloectx redact` does once the daemon has
    // let go of it).
    drop(d);
    let f = chloectx::daemon::rewrite_file(dir.path(), std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(f["records"], json!(3), "{f}");
    for s in [&pw, &gh, &ant] {
        assert!(!file_holds(dir.path(), s), "the store's files still hold a secret");
    }
    let d = daemon(dir.path());
    assert_eq!(all_text(&d), after);

    // A second run finds nothing left.
    let r = call(&d, json!({"op": "redact"}));
    assert_eq!((r["changed"].as_u64(), r["sessions"].as_u64(), r["log_lines"].as_u64()), (Some(0), Some(0), Some(0)), "{r}");

    // What the hooks capture from now on is redacted before it is stored,
    // the prompt in the session state and the turn's context too.
    call(&d, json!({"op": "hook", "event": "UserPromptSubmit", "payload": {"session_id": "s1", "prompt": format!("why does curl -H 'Authorization: Bearer {gh}' fail with DB_PASSWORD={pw}?")}}));
    call(&d, json!({"op": "hook", "event": "PostToolUse", "payload": {"session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "cat .env"},
        "tool_response": {"stdout": format!("DB_PASSWORD={pw}\nANTHROPIC_API_KEY={ant}\nDATABASE_URL=postgres://app:{pw}@db/app"), "stderr": ""}}}));
    call(&d, json!({"op": "hook", "event": "PostToolUse", "payload": {"session_id": "s1", "tool_name": "mcp__x__deploy", "tool_input": {"env": {"DB_PASSWORD": pw}}, "tool_response": "ok"}}));
    call(&d, json!({"op": "remember", "kind": "fact", "text": format!("the staging token is {gh}")}));
    call(&d, json!({"op": "hook", "event": "Stop", "payload": {"session_id": "s1", "last_assistant_message": format!("Set DB_PASSWORD={pw} and it worked.")}}));
    d.persist();
    let now = all_text(&d);
    for s in [&pw, &gh, &ant] {
        assert!(!now.contains(s.as_str()), "{now}");
    }
    assert!(now.contains("Turn 1 request: why does curl -H 'Authorization: Bearer [REDACTED:github-token]'"), "{now}");
    assert!(now.contains("\"DB_PASSWORD\":\"[REDACTED:password]\""), "{now}");
    let state = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
    for s in [&pw, &gh] {
        assert!(!state.contains(s.as_str()), "{state}");
    }
}
