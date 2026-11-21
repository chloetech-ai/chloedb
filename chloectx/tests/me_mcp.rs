//! `chloectx me mcp`, as Claude Desktop's extension runs it: MCP on stdin
//! and stdout, finding an export in Downloads, importing it, drafting the
//! persona card from the person's own messages (only lines that name them
//! are kept), and confirming, changing and dropping lines as the person
//! says. The hash embedder, so no model files.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

struct Mcp {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    n: u64,
}

impl Mcp {
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params})).unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert!(v.get("error").is_none(), "{method}: {v}");
        v["result"].clone()
    }

    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        let r = self.call("tools/call", json!({"name": name, "arguments": args}));
        (r["content"][0]["text"].as_str().unwrap_or("").to_string(), r["isError"] == true)
    }

    fn ok(&mut self, name: &str, args: Value) -> String {
        let (t, e) = self.tool(name, args);
        assert!(!e, "{name}: {t}");
        t
    }
}

#[test]
fn claude_desktop_imports_drafts_and_reviews() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("Downloads/claude-data")).unwrap();
    let convo = json!([{"uuid": "c-1", "name": "Marathon plan", "created_at": "2025-04-01T10:00:00Z", "chat_messages": [
        {"sender": "human", "text": "I'm training for the Lisbon marathon and I run before work.", "created_at": "2025-04-01T10:00:00Z"},
        {"sender": "assistant", "text": "Build up your long run slowly.", "created_at": "2025-04-01T10:00:05Z"},
        {"sender": "human", "text": "I'm vegetarian, so plan meals without meat.", "created_at": "2025-04-01T10:01:00Z"}]}]);
    std::fs::write(home.join("Downloads/claude-data/conversations.json"), convo.to_string()).unwrap();
    let bin = env!("CARGO_BIN_EXE_chloectx");
    let mut child = Command::new(bin)
        .args(["me", "mcp", "--client", "Claude"])
        .env("HOME", &home)
        .env("CHLOECTX_HOME", dir.path().join("ctx"))
        .env("CHLOECTX_EMBEDDER", "hash:64")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let out = BufReader::new(child.stdout.take().unwrap());
    let mut m = Mcp { child, out, n: 0 };
    let init = m.call("initialize", json!({"protocolVersion": "2025-06-18"}));
    assert!(init["instructions"].as_str().unwrap().contains("find_exports"), "{init}");
    let names: Vec<String> = m.call("tools/list", json!({}))["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["persona", "search", "fetch", "ask", "remember", "persona_draft", "persona_suggest", "persona_review", "find_exports", "import"]);

    // Found in Downloads, imported once.
    let found: Value = serde_json::from_str(&m.ok("find_exports", json!({}))).unwrap();
    let path = found["exports"][0]["path"].as_str().unwrap().to_string();
    assert!(path.ends_with("claude-data"), "{found}");
    assert!(m.ok("import", json!({"path": path})).contains("3 new message"));
    assert!(m.ok("import", json!({"path": "~/Downloads/claude-data"})).contains("0 new message"), "a second import adds nothing");
    let (said, err) = m.tool("import", json!({"path": "~/Downloads/nothing-here"}));
    assert!(err && said.contains("nothing-here"), "{said}");

    // The draft gives the person's own messages, never the assistant's.
    let draft = m.ok("persona_draft", json!({}));
    assert!(draft.contains("Part 1 of 1") && draft.contains("Lisbon marathon") && !draft.contains("Build up your long run"), "{draft}");
    let ids: Vec<String> = draft.split("message id=\"").skip(1).map(|s| s.split('"').next().unwrap().to_string()).collect();
    assert_eq!(ids.len(), 2, "{draft}");
    let said = m.ok("persona_suggest", json!({"lines": [
        {"section": "Interests", "text": "Training for the Lisbon marathon; runs before work.", "sources": [ids[0]]},
        {"section": "Health & diet", "kind": "preference", "text": "Vegetarian.", "sources": [ids[1]]},
        {"section": "Travel", "text": "Lives on the moon.", "sources": ["made:1"]},
        {"section": "Health & diet", "text": "vegetarian", "sources": [ids[1]]}]}));
    assert!(said.contains("Added 2") && said.contains("1 left out") && said.contains("1 already"), "{said}");
    assert!(m.ok("persona_draft", json!({"part": 2})).contains("all were given"));
    let card = m.ok("persona", json!({}));
    assert!(card.contains("Vegetarian. (suggested)") && card.contains("Lisbon marathon"), "{card}");

    // The person's word: confirm one, change one.
    let line_id = |card: &str, words: &str| card.lines().find(|l| l.contains(words)).and_then(|l| l.split('[').nth(1)).map(|s| s.split(']').next().unwrap().to_string()).unwrap();
    let veg = line_id(&card, "Vegetarian.");
    let run = line_id(&card, "Lisbon");
    m.ok("persona_review", json!({"confirm": [veg], "change": [{"id": run, "text": "Runs the Lisbon marathon in March."}]}));
    let card = m.ok("persona", json!({}));
    assert!(card.contains("Vegetarian.") && !card.contains("Vegetarian. (suggested)") && card.contains("Runs the Lisbon marathon in March.") && !card.contains("runs before work"), "{card}");
    let ran = line_id(&card, "March");
    m.ok("persona_review", json!({"drop": [ran]}));
    assert!(!m.ok("persona", json!({})).contains("March"));

    drop(m.child.stdin.take());
    let _ = m.child.wait();
    let _ = Command::new(bin).args(["stop", "--all"]).env("HOME", &home).env("CHLOECTX_HOME", dir.path().join("ctx")).output();
}
