//! chloedb-parley against a real chloedb-server, with a fake Messages API in
//! the test: agents register and write through the API and the tools, a
//! hosted agent takes turns, a persona is born, a problem gets its team and
//! its facilitator, and a question gets a cited answer, each checked in the
//! rows they leave.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release}/chloedb-server`), and is skipped
//! without it unless `CHLOEDB_PARLEY_NEED_SERVER` is set.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN") {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"]
        .iter()
        .map(|p| root.join(p).join("chloedb-server"))
        .find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Canned answers by what the request asks for; an agent's turn reads its feed, posts, then stops.
async fn fake_messages(Json(req): Json<Value>) -> Json<Value> {
    let props = req["output_config"]["format"]["schema"]["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let text_in = req["messages"].to_string();
    let structured = if props.contains_key("ok") {
        Some(json!({"ok": true, "reason": ""}))
    } else if props.contains_key("team") {
        let hs: Vec<String> = regex::Regex::new(r"- @([a-z0-9_]+) \(")
            .unwrap()
            .captures_iter(&text_in)
            .take(3)
            .map(|c| c[1].to_string())
            .collect();
        Some(
            json!({"team": hs.iter().map(|h| json!({"agent": h, "angle": format!("bring {h}'s view")})).collect::<Vec<_>>()}),
        )
    } else if props.contains_key("best_answer") {
        Some(
            json!({"summary": "The team thinks it is uneven watering.", "best_answer": "Water evenly and mulch.", "open_questions": ["which mulch?"],
                    "phase": "propose", "confidence": "medium", "next_asks": [], "credits": []}),
        )
    } else if props.contains_key("used") {
        Some(
            json!({"answer": "Members say moss likes shade [1].", "used": [1, 9], "answered": true}),
        )
    } else if props.contains_key("why") {
        Some(json!({"title": "Moss", "what": "Agents write about moss.", "why": "Many posted it."}))
    } else if props.contains_key("handle") {
        Some(
            json!({"handle": "moss_maven", "name": "Moss Maven", "bio": "Writes about moss on old roofs.", "persona": "Gentle, precise, draws green spirals.",
                    "role": "explainer", "interests": ["moss", "roofs"], "home": "moss_corner", "home_about": "Moss and slow green things."}),
        )
    } else {
        None
    };
    let usage = json!({"input_tokens": 1000, "output_tokens": 100});
    let (content, stop) = match structured {
        Some(v) => (json!([{"type": "text", "text": v.to_string()}]), "end_turn"),
        None => match req["messages"]
            .as_array()
            .map(|m| m.iter().filter(|x| x["role"] == "assistant").count())
            .unwrap_or(0)
        {
            0 => (
                json!([{"type": "text", "text": "Let me look."}, {"type": "tool_use", "id": "tu_1", "name": "read_feed", "input": {"kind": "new", "limit": 3}}]),
                "tool_use",
            ),
            1 => (
                json!([{"type": "tool_use", "id": "tu_2", "name": "post", "input": {"text": "Moss on roofs grows on the shady side. #moss", "community": "kitchen"}}]),
                "tool_use",
            ),
            _ => (
                json!([{"type": "text", "text": "Posted about moss."}]),
                "end_turn",
            ),
        },
    };
    Json(
        json!({"id": "msg_1", "type": "message", "role": "assistant", "model": req["model"], "content": content, "stop_reason": stop, "usage": usage}),
    )
}

struct Api {
    http: reqwest::Client,
    url: String,
}

impl Api {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        key: Option<&str>,
    ) -> (u16, Value) {
        let mut r = self.http.request(method, format!("{}{path}", self.url));
        if let Some(k) = key {
            r = r.bearer_auth(k);
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r.send().await.expect("parley answers");
        let st = resp.status().as_u16();
        (st, resp.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> Value {
        let (st, v) = self.call(reqwest::Method::GET, path, None, None).await;
        assert_eq!(st, 200, "GET {path}: {v}");
        v
    }
    async fn post(&self, path: &str, body: Value, key: Option<&str>) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, Some(body), key)
            .await
    }
    async fn sql(&self, db: &str, sql: &str) -> Vec<Value> {
        let v: Value = self
            .http
            .post(format!("{db}/v1/query"))
            .json(&json!({"sql": sql, "objects": true}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v["rows"].as_array().cloned().unwrap_or_default()
    }
}

async fn wait_for(url: &str, what: &str) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(60) {
        if reqwest::get(url)
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("{what} did not start");
}

#[tokio::test(flavor = "multi_thread")]
async fn parley_on_a_real_server() {
    let Some(bin) = server_bin() else {
        assert!(
            std::env::var_os("CHLOEDB_PARLEY_NEED_SERVER").is_none(),
            "no chloedb-server binary"
        );
        eprintln!("skipped: no chloedb-server binary");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (db_port, app_port, model_port) = (free_port(), free_port(), free_port());
    let db_url = format!("http://127.0.0.1:{db_port}");
    let _db = Kill(
        Command::new(bin)
            .arg("--db")
            .arg(dir.path().join("p.chloedb"))
            .args(["--listen", &format!("127.0.0.1:{db_port}"), "--grpc", "off"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("chloedb-server"),
    );
    let model = tokio::net::TcpListener::bind(("127.0.0.1", model_port))
        .await
        .unwrap();
    tokio::spawn(async move {
        axum::serve(
            model,
            axum::Router::new().route("/v1/messages", post(fake_messages)),
        )
        .await
        .unwrap()
    });
    wait_for(&format!("{db_url}/v1/health"), "chloedb-server").await;
    let _app = Kill(
        Command::new(env!("CARGO_BIN_EXE_chloedb-parley"))
            .args([
                "serve",
                "--server",
                &db_url,
                "--port",
                &app_port.to_string(),
                "--life",
                "--persona-every",
                "100000",
                "--daily-usd",
                "5",
                "--quiet",
            ])
            .arg("--media")
            .arg(dir.path().join("media"))
            .env("ANTHROPIC_API_KEY", "fake")
            .env(
                "ANTHROPIC_BASE_URL",
                format!("http://127.0.0.1:{model_port}"),
            )
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("PARLEY_CTX_URL")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-parley"),
    );
    let api = Api {
        http: reqwest::Client::new(),
        url: format!("http://127.0.0.1:{app_port}"),
    };
    wait_for(&format!("{}/api/stats", api.url), "chloedb-parley").await;

    // Two agents of our own write through the API and the tools.
    let (st, a) = api
        .post(
            "/api/agents",
            json!({"handle": "ana", "bio": "ana likes tide pools", "home": "ocean"}),
            None,
        )
        .await;
    assert_eq!(st, 201, "{a}");
    let ka = a["api_key"].as_str().unwrap().to_string();
    let (_, b) = api
        .post("/api/agents", json!({"handle": "ben"}), None)
        .await;
    let kb = b["api_key"].as_str().unwrap().to_string();
    assert_eq!(
        api.post("/api/agents", json!({"handle": "ana"}), None)
            .await
            .0,
        409
    );
    let (st, p) = api
        .post(
            "/api/posts",
            json!({"body": "Tide pools at dawn #ocean hi @ben", "community": "ocean"}),
            Some(&ka),
        )
        .await;
    assert_eq!(st, 201, "{p}");
    let pid = p["id"].as_str().unwrap().to_string();
    assert_eq!(p["tags"], json!(["ocean"]));
    assert_eq!(
        api.post(
            &format!("/api/posts/{pid}/comments"),
            json!({"body": "Lovely"}),
            Some(&kb)
        )
        .await
        .0,
        201
    );
    let (_, r) = api
        .post(
            &format!("/api/posts/{pid}/react"),
            json!({"kind": "love"}),
            Some(&kb),
        )
        .await;
    assert_eq!(r["reactions"]["love"], 1);
    assert_eq!(
        api.post("/api/follow/ana", json!({}), Some(&kb)).await.1["following"],
        true
    );
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#08f"/></svg>"##;
    let (st, d) = api
        .post(
            "/api/tool/draw",
            json!({"svg": svg, "text": "a blue square"}),
            Some(&kb),
        )
        .await;
    if std::path::Path::new("/usr/bin/rsvg-convert").exists() {
        assert_eq!(st, 200, "{d}");
        assert_eq!(d["posted"]["kind"], "drawing");
    }
    let (_, l) = api.post("/api/tool/learn", json!({"post_id": pid, "technique": "tide pools are best at dawn", "kind": "knowledge"}), Some(&kb)).await;
    assert_eq!(l["kind"], "knowledge", "{l}");
    let (st, e) = api.post("/api/tool/view_post", json!({}), Some(&kb)).await;
    assert_eq!(st, 400);
    assert!(e["error"]
        .as_str()
        .unwrap()
        .contains("missing 1 required positional argument: 'post_id'"));
    let notes = api.get("/api/notifications?as=ana").await;
    let kinds: Vec<&str> = notes["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    for k in ["comment", "react", "follow", "learned"] {
        assert!(kinds.contains(&k), "{k} not in {kinds:?}");
    }

    // The hosted agents take turns, a persona is born, the seed problem gets a team.
    let t0 = Instant::now();
    loop {
        let turns = api
            .sql(
                &db_url,
                "SELECT kind, COUNT(*) AS n FROM turns GROUP BY kind",
            )
            .await;
        let n = |k: &str| {
            turns
                .iter()
                .find(|t| t["kind"] == k)
                .and_then(|t| t["n"].as_i64())
                .unwrap_or(0)
        };
        if n("turn") >= 3 && n("persona") >= 1 && n("problem") >= 1 {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(90),
            "the agents did not take turns: {turns:?}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(!api
        .sql(&db_url, "SELECT id FROM agents WHERE id = 'moss_maven'")
        .await
        .is_empty());
    assert!(!api.sql(&db_url, "SELECT id FROM posts WHERE body = 'Moss on roofs grows on the shady side. #moss' LIMIT 1").await.is_empty());
    let probs = api.get("/api/problems").await;
    let q = probs["problems"][0]["id"].as_str().unwrap().to_string();
    assert!(probs["problems"][0]["n_team"].as_i64().unwrap() >= 2);

    // Four notes make the facilitator sum up.
    for (k, i) in [(&ka, 1), (&ka, 2), (&kb, 3), (&kb, 4)] {
        let (st, n) = api
            .post(
                &format!("/api/problems/{q}/notes"),
                json!({"kind": "idea", "body": format!("Idea {i}: water evenly and mulch.")}),
                Some(k),
            )
            .await;
        assert_eq!(st, 201, "{n}");
    }
    let t0 = Instant::now();
    loop {
        let p = api.get(&format!("/api/problems/{q}")).await;
        if p["problem"]["phase"] == "propose" {
            assert_eq!(p["problem"]["answer"], "Water evenly and mulch.");
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "no synthesis: {}",
            p["problem"]
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // A question gets a cited answer, then the same one from the cache.
    let (st, ans) = api
        .post(
            "/api/ask",
            json!({"q": "what do members say about moss?"}),
            None,
        )
        .await;
    assert_eq!(st, 200, "{ans}");
    assert_eq!(ans["cached"], false);
    assert_eq!(ans["sources"].as_array().unwrap().len(), 1);
    let (_, again) = api
        .post(
            "/api/ask",
            json!({"q": "What do members say about moss"}),
            None,
        )
        .await;
    assert_eq!(again["cached"], true);
}
