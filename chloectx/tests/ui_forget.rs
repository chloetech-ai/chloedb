//! JONAH-206: the inspector forgets a record from a button. The real
//! `chloectx ui`, in a home of the test's own, over a store the test
//! writes: the page has the control, the server lists what would go
//! before it deletes anything, forgets only when the page itself asks,
//! and the record is gone from the store afterwards.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use chloectx::daemon::Daemon;
use serde_json::{json, Value as Json};

struct Ui {
    child: Child,
    addr: String,
    token: String,
}

impl Drop for Ui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Ui {
    /// One HTTP request; the status code and the body.
    fn http(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
        // The token in the cookie the page's first load sets.
        let port = self.addr.rsplit(':').next().unwrap();
        let mut s = TcpStream::connect(&self.addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nCookie: engram_ui_{port}={}\r\nConnection: close\r\nContent-Length: {}\r\n",
            self.addr,
            self.token,
            body.len()
        );
        for (k, v) in headers {
            req += &format!("{k}: {v}\r\n");
        }
        req += "\r\n";
        req += body;
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let code = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
        (code, body)
    }

    fn forget(&self, body: Json, from_page: bool) -> (u16, Json) {
        let mut h = vec![("Content-Type", "application/json")];
        if from_page {
            h.push(("X-ChloeCtx-UI", "1"));
        }
        let (code, b) = self.http("POST", "/api/forget", &h, &body.to_string());
        (code, serde_json::from_str(&b).unwrap_or(Json::Null))
    }

    fn sql(&self, sql: &str) -> Json {
        let (_, b) = self.http("POST", "/api/rpc", &[("Content-Type", "application/json")], &json!({"op": "inspect_query", "sql": sql}).to_string());
        serde_json::from_str(&b).unwrap()
    }
}

#[test]
fn the_inspector_forgets_a_record_from_a_button() {
    let t = tempfile::tempdir().unwrap();
    let (ctx, home, proj) = (t.path().join("ctx"), t.path().join("home"), t.path().join("proj"));
    for d in [&ctx, &home, &proj] {
        std::fs::create_dir_all(d).unwrap();
    }
    let proj = proj.canonicalize().unwrap();
    let store = ctx.join("projects").join(chloectx::paths::project_slug(&proj));
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("config.json"), r#"{"embedder": "hash:64", "sync": false, "distill": {"enabled": false}}"#).unwrap();
    std::fs::write(store.join("project.txt"), format!("{}\n", proj.display())).unwrap();
    let keep;
    let gone;
    {
        let d = Arc::new(Daemon::open(&store, proj.to_str().unwrap()).unwrap());
        let call = |req: Json| {
            let (r, later) = d.handle(&req);
            if let Some(f) = later {
                f();
            }
            assert!(r.get("error").is_none(), "{r}");
            r
        };
        gone = call(json!({"op": "remember", "kind": "decision", "text": "Invoices are rounded to whole cents before tax."}))["id"].as_str().unwrap().to_string();
        keep = call(json!({"op": "remember", "kind": "decision", "text": "The CSV export uses semicolons."}))["id"].as_str().unwrap().to_string();
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_chloectx"))
        .args(["ui", "--port", "0", "--no-open", "--home"])
        .arg(&ctx)
        .current_dir(&proj)
        .env("HOME", &home)
        .env("CHLOECTX_HOME", &ctx)
        .env("CHLOECTX_EMBEDDER", "hash:64")
        .env("CHLOECTX_SOURCE", "/nonexistent")
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("CHLOECTX_DISTILL_MODEL")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let err = BufReader::new(child.stderr.take().unwrap());
    let url = err.lines().map_while(|l| l.ok()).find_map(|l| l.split_once("open http://").map(|(_, u)| u.to_string())).expect("the URL it prints");
    let (addr, token) = url.split_once("/?token=").unwrap();
    let ui = Ui { child, addr: addr.to_string(), token: token.to_string() };

    // The page has the control.
    let (code, page) = ui.http("GET", "/", &[], "");
    assert_eq!(code, 200);
    assert!(page.contains("Forget…") && page.contains("/api/forget"), "the page has no forget control");

    // Only the page itself may ask (a form post from another site cannot).
    let (code, r) = ui.forget(json!({"ids": [gone]}), false);
    assert_eq!(code, 403, "{r}");

    // The dry run lists what would go and deletes nothing.
    let (code, r) = ui.forget(json!({"ids": [gone], "dry_run": true}), true);
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["total"], 1, "{r}");
    assert!(r["matches"][0]["line"].as_str().unwrap().contains("whole cents"), "{r}");
    let (code, r) = ui.forget(json!({"text": "whole cents", "dry_run": true}), true);
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["total"], 1, "{r}");
    let n = |id: &str| ui.sql(&format!("SELECT id FROM memory WHERE id = '{id}' LIMIT 1"))["rows"].as_array().map(|a| a.len()).unwrap_or(99);
    assert_eq!(n(&gone), 1, "a dry run deletes nothing");

    // Forget: the record is gone, the other stays.
    let (code, r) = ui.forget(json!({"ids": [gone]}), true);
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["deleted"], json!([gone]), "{r}");
    assert_eq!(n(&gone), 0);
    assert_eq!(n(&keep), 1);
    let (code, _) = ui.forget(json!({}), true);
    assert_eq!(code, 400);

    drop(ui);
    let _ = chloectx::daemon::request_running(&store, &json!({"op": "shutdown"}), Duration::from_secs(10));
}
