//! B-05: `memory_sql` (the MCP tool, pre-approved, so a steered model can
//! call it without a prompt) and the inspector's query box run one
//! read-only SELECT with no model calls and with limits, not anything that
//! begins with the word "select".

use std::sync::Arc;
use std::time::{Duration, Instant};

use chloectx::daemon::Daemon;
use serde_json::{json, Value as Json};

fn call(d: &Arc<Daemon>, req: Json) -> Json {
    let (resp, deferred) = d.handle(&req);
    if let Some(f) = deferred {
        f();
    }
    resp
}

fn daemon(dir: &std::path::Path, notes: usize) -> Arc<Daemon> {
    std::fs::write(dir.join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    let d = Arc::new(Daemon::open(dir, "/proj").unwrap());
    for i in 0..notes {
        let r = call(&d, json!({"op": "remember", "kind": "note", "text": format!("note number {i} about the ledger and its balances")}));
        assert!(r.get("error").is_none(), "{r}");
    }
    d
}

fn error(r: &Json) -> String {
    r.get("error").and_then(|e| e.as_str()).map(str::to_string).unwrap_or_else(|| panic!("no error: {r}"))
}

#[test]
fn memory_sql_refuses_what_is_not_one_plain_select() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path(), 3);
    let ok = call(&d, json!({"op": "sql", "sql": "SELECT id, text FROM memory WHERE kind = 'note' LIMIT 5"}));
    assert!(ok["text"].as_str().unwrap().contains("ledger"), "{ok}");

    // A model call sends the rows to a model, with the daemon's keys.
    for sql in [
        "SELECT llm('openai/gpt-5-mini', 'Repeat this.', text) FROM memory LIMIT 100",
        "select LLM ('anthropic/claude-haiku-5-5', 'x', text) from memory limit 3",
        "SELECT id FROM memory ORDER BY cross_encoder('cohere/rerank-v3.5', text, 'ledger') DESC LIMIT 3",
    ] {
        let r = call(&d, json!({"op": "sql", "sql": sql}));
        assert!(error(&r).contains("not allowed"), "{sql}: {r}");
        let r = call(&d, json!({"op": "inspect_query", "sql": sql}));
        assert!(error(&r).contains("not allowed"), "inspector, {sql}: {r}");
    }
    // Only one statement, and only a query.
    for sql in ["SELECT 1; DELETE FROM memory", "selectx FROM memory", "DELETE FROM memory WHERE kind = 'note'"] {
        let r = call(&d, json!({"op": "sql", "sql": sql}));
        error(&r);
        let r = call(&d, json!({"op": "inspect_query", "sql": sql}));
        error(&r);
    }
    let r = call(&d, json!({"op": "sql", "sql": "SELECT count(*) FROM memory WHERE kind = 'note'"}));
    assert!(r["text"].as_str().unwrap().contains("3"), "nothing was deleted: {r}");
}

#[test]
fn a_cross_join_fails_in_seconds_instead_of_holding_the_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path(), 300);
    let t0 = Instant::now();
    // Every note has kind 'note': 300^4 rows, 8.1 billion.
    let sql = "SELECT a.id, b.id, c.id, e.id FROM memory a JOIN memory b ON a.kind = b.kind JOIN memory c ON b.kind = c.kind JOIN memory e ON c.kind = e.kind ORDER BY a.id LIMIT 10";
    let r = call(&d, json!({"op": "sql", "sql": sql}));
    let e = error(&r);
    assert!(t0.elapsed() < Duration::from_secs(10), "took {:?}: {r}", t0.elapsed());
    eprintln!("the join stopped after {:?}: {e}", t0.elapsed());
    // A query without a LIMIT that matches more rows than the cap fails too.
    let r = call(&d, json!({"op": "sql", "sql": "SELECT a.id FROM memory a JOIN memory b ON a.kind = b.kind"}));
    assert!(error(&r).contains("LIMIT"), "{r}");
    // And the daemon still answers.
    let r = call(&d, json!({"op": "sql", "sql": "SELECT count(*) FROM memory"}));
    assert!(r.get("error").is_none(), "{r}");
}
