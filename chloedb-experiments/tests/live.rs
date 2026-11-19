//! A live run against a real chloedb-server: simulated users at a rate,
//! their searches logged and their clicks written back, every metric each
//! second; AI agents played through a stand-in of the Messages API (so
//! no model is called, and what the app sends it is checked), split
//! evenly over the arms and stopped by their budget; a visitor's search;
//! the rate changed while it runs; once stopped, the run as an experiment
//! with its report and a sentence per arm on why its agents gave up. And a
//! second run that stops itself once it is decided.
//!
//! It needs the chloedb-server binary, as tests/app.rs does.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use chloedb_experiments::app::{self, Opts};
use common::Client;

type Seen = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

/// A stand-in of the Messages API: it searches the need's own words,
/// opens the first result and says it found it, at 1,000 tokens in and
/// 200 out a call.
async fn messages(State(seen): State<Seen>, headers: HeaderMap, Json(b): Json<Value>) -> (StatusCode, Json<Value>) {
    seen.lock().unwrap().push((headers.clone(), b.clone()));
    let bad = |m: &str| (StatusCode::BAD_REQUEST, Json(json!({"type": "error", "error": {"type": "invalid_request_error", "message": m}})));
    if headers.get("x-api-key").and_then(|v| v.to_str().ok()) != Some("k") {
        return (StatusCode::UNAUTHORIZED, Json(json!({"type": "error", "error": {"type": "authentication_error", "message": "bad key"}})));
    }
    if headers.get("anthropic-version").and_then(|v| v.to_str().ok()) != Some("2026-10-21") {
        return bad("anthropic-version");
    }
    if b["model"] != "claude-opus-5-5" || b["fallbacks"] != "default" || !headers.get("anthropic-beta").and_then(|v| v.to_str().ok()).unwrap_or("").contains("server-side-fallback-2026-07-01") {
        return bad("the model, or its fallbacks");
    }
    if b["output_config"]["effort"] != "low" {
        return bad("effort");
    }
    let msgs = b["messages"].as_array().cloned().unwrap_or_default();
    // No tools: the sum-up of why the agents gave up, a line per arm named.
    if b["tools"].is_null() {
        let prompt = msgs[0]["content"].as_str().unwrap_or("");
        let lines: Vec<String> = (0..5).filter(|a| prompt.contains(&format!("\nArm {a} ("))).map(|a| format!("{a}: the rows it showed were about something else")).collect();
        return (StatusCode::OK, Json(json!({"id": "msg_s", "type": "message", "role": "assistant", "model": "claude-opus-5-5", "stop_reason": "end_turn", "content": [{"type": "text", "text": lines.join("\n")}], "usage": {"input_tokens": 500, "output_tokens": 50}})));
    }
    if b["tool_choice"]["type"] != "auto" {
        return bad("tool_choice");
    }
    let first = msgs[0]["content"].as_str().unwrap_or("");
    let need = first.split("looking for this:\n\n\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
    let turns = msgs.iter().filter(|m| m["role"] == "assistant").count();
    // Each answer of a tool must answer the call before it.
    if turns > 0 {
        let last = msgs.last().unwrap();
        let prev = &msgs[msgs.len() - 2];
        if last["content"][0]["type"] != "tool_result" || last["content"][0]["tool_use_id"] != prev["content"][1]["id"] {
            return bad("a tool_result for the tool_use before");
        }
    }
    let call = match turns {
        0 => json!({"name": "search", "input": {"query": need, "why": "my words"}}),
        1 => json!({"name": "open", "input": {"result": 1, "why": "the first looks right"}}),
        // A need of odd length is given up on.
        _ if need.len() % 2 == 1 => json!({"name": "done", "input": {"found": false, "note": "nothing on it"}}),
        _ => json!({"name": "done", "input": {"found": true, "result": 1, "note": "that was it"}}),
    };
    let id = format!("toolu_{}_{turns}", need.len());
    (
        StatusCode::OK,
        Json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5", "stop_reason": "tool_use",
            "content": [{"type": "thinking", "thinking": "", "signature": "sig"}, {"type": "tool_use", "id": id, "name": call["name"], "input": call["input"]}],
            "usage": {"input_tokens": 1000, "output_tokens": 200},
        })),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_live_run_counts_every_second_and_ends_as_an_experiment() {
    let Some((server, _guard, _dir)) = common::shop_server().await else { return };

    // The stand-in of the Messages API.
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let fake = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", fake.local_addr().unwrap());
    let r = Router::new().route("/v1/messages", post(messages)).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(fake, r).await.unwrap() });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(app::serve(server.clone(), listener, Opts { token: Some("t".into()), beir: Vec::new(), anthropic_key: Some("k".into()), anthropic_base: Some(base), sources: Vec::new() }));
    let c = Client { url, http: reqwest::Client::new() };
    common::ready(&c).await;
    c.post("/v1/sites", common::shop_site()).await;
    let meta = c.get("/v1/meta").await;
    assert_eq!(meta["live"]["claude"], true);
    assert_eq!(meta["live"]["defaults"]["agent_model"], "claude-opus-5-5");

    // Refused: an agent model the app does not offer.
    let arms = json!([{"name": "Body", "rank": common::BODY}, {"name": "Title", "rank": common::TITLE}]);
    let (s, v) = c.call(reqwest::Method::POST, "/v1/live", Some(json!({"name": "x", "site": "shop", "arms": arms, "agents": 1, "agent_model": "gpt-9"})), Some("t")).await;
    assert_eq!(s, 400, "{v}");

    // Two agents at $0.008 a call ($4 and $20 a million tokens), and a
    // budget of six cents: two tasks of three calls each, or near.
    let spec = json!({"name": "live title vs body", "site": "shop", "arms": arms, "users": 2000, "rate": 80, "minutes": 5, "agents": 2, "budget": 0.06, "until": "time"});
    let id = c.post("/v1/live", spec.clone()).await["id"].as_str().unwrap().to_string();
    let (s, v) = c.call(reqwest::Method::POST, "/v1/live", Some(spec), Some("t")).await;
    assert_eq!(s, 409, "one live run at a time: {v}");

    // It goes live, and counts.
    let t0 = Instant::now();
    let mut v;
    loop {
        v = c.get(&format!("/v1/live/{id}")).await;
        assert_ne!(v["status"], "failed", "{}", v["error"]);
        let agents_done = v["agents"].as_array().map(|a| a.len() == 2 && a.iter().all(|x| x["status"] == "budget spent")).unwrap_or(false);
        if v["status"] == "live" && v["searches"].as_u64().unwrap_or(0) >= 300 && agents_done {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(90), "the live run did not get going: {}", serde_json::to_string(&v).unwrap().chars().take(2000).collect::<String>());
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let metrics = v["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 9, "the eight metrics and the judged quality");
    let success = metrics.iter().find(|m| m["key"] == "success").unwrap();
    assert!(success["arms"][0]["value"].as_f64().unwrap() > 0.0);
    assert!(success["arms"][1]["truth"].as_f64().unwrap() > success["arms"][0]["truth"].as_f64().unwrap(), "the title ranking is truly better here: {success}");
    let cmp = &success["comparisons"][0];
    for k in ["delta", "lo", "hi", "p", "p_always_valid", "lift", "true_delta", "mde", "power"] {
        assert!(cmp[k].is_number(), "{k}: {cmp}");
    }
    assert!(cmp["p_always_valid"].as_f64().unwrap() >= cmp["p"].as_f64().unwrap() * 0.999, "{cmp}");
    let ndcg = metrics.iter().find(|m| m["key"] == "ndcg").unwrap();
    assert!(ndcg["arms"][1]["truth"].as_f64().unwrap() > ndcg["arms"][0]["truth"].as_f64().unwrap(), "{ndcg}");
    assert_eq!(v["verdicts"].as_array().unwrap().len(), 1);
    assert!(v["srm"]["ok"].is_boolean());
    let points = v["points"].as_array().unwrap();
    assert!(!points.is_empty() && points[0]["d"].as_array().unwrap().len() == 9);
    let last = v["points_last"].as_u64().unwrap();
    let feed = v["feed"].as_array().unwrap();
    assert!(feed.iter().any(|f| f["kind"] == "user"), "the feed shows simulated users");
    assert!(feed.iter().any(|f| f["kind"] == "agent" && f["action"] == "open"), "and the agents");

    // The agents: every task three calls, the first result opened (found
    // when the need is of an even length, the fake says);
    // the budget spent, and not by much more.
    let tally: Vec<Value> = v["agent_tally"].as_array().unwrap().clone();
    let tasks: u64 = tally.iter().map(|t| t["tasks"].as_u64().unwrap()).sum();
    let found: u64 = tally.iter().map(|t| t["found"].as_u64().unwrap()).sum();
    assert!(tasks >= 2 && found <= tasks, "{tally:?}");
    assert!(tally.iter().all(|t| t["tasks"].as_u64().unwrap() >= 1), "the agents are split over the arms: {tally:?}");
    let arms_of: Vec<Value> = v["agents"].as_array().unwrap().iter().map(|a| a["arm"].clone()).collect();
    assert_ne!(arms_of[0], arms_of[1], "two agents, one in each arm: {arms_of:?}");
    // The budget is spent but for what the sum-up keeps (a tenth of it here).
    let cost = v["cost"].as_f64().unwrap();
    assert!((0.054..=0.06 + 2.0 * 0.008 + 1e-9).contains(&cost), "spent ${cost}");
    let calls = seen.lock().unwrap().len();
    assert!((cost / 0.008 - calls as f64).abs() < 1e-6, "{calls} calls cost ${cost}");
    let agent = &v["agents"][0];
    assert!(agent["persona"]["short"].as_str().unwrap().contains(" · "));
    assert!(agent["last"]["need"].is_string(), "{agent}");
    // Each first message names the person and the need.
    let first = seen.lock().unwrap()[0].1["messages"][0]["content"].as_str().unwrap().to_string();
    assert!(first.starts_with("You are a ") && first.contains("aged") && first.contains("a test shop"), "{first}");

    // Only what came since.
    let next = c.get(&format!("/v1/live/{id}?after={last}&since={}", v["seq"])).await;
    assert!(next["points"].as_array().unwrap().iter().all(|p| p["id"].as_u64().unwrap() > last));
    assert!(next["feed"].as_array().unwrap().iter().all(|f| f["seq"].as_u64().unwrap() > v["seq"].as_u64().unwrap()));

    // A visitor: a search, a result opened, and found.
    let page = c.post(&format!("/v1/live/{id}/try"), json!({"who": "tester", "q": "tent lamp"})).await;
    assert!(page["rows"].as_array().unwrap().len() == 5, "{page}");
    let request = page["request"].as_str().unwrap().to_string();
    let doc = c.post(&format!("/v1/live/{id}/try/open"), json!({"request": request, "result": 2})).await;
    assert!(doc["body"].as_str().unwrap().len() > 3, "{doc}");
    let done = c.post(&format!("/v1/live/{id}/try/done"), json!({"request": request, "result": 2, "found": true})).await;
    assert_eq!(done["found"], true);
    let (s, _) = c.call(reqwest::Method::POST, &format!("/v1/live/{id}/try/open"), Some(json!({"request": request, "result": 1})), Some("t")).await;
    assert_eq!(s, 400, "a page is done with once found");

    // The rate, changed while it runs.
    assert_eq!(c.post(&format!("/v1/live/{id}/rate"), json!({"rate": 20})).await["rate"], 20.0);
    let (s, _) = c.call(reqwest::Method::POST, &format!("/v1/live/{id}/rate"), Some(json!({"rate": 1e9})), Some("t")).await;
    assert_eq!(s, 400);

    // The cohorts so far: each cell's users are those who searched.
    let co = c.post(&format!("/v1/live/{id}/cohorts"), json!({"by": ["device"]})).await;
    let cells = co["metrics"]["success"][0]["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 3, "{co}");
    let seen: u64 = cells.iter().map(|x| x["users"].as_u64().unwrap()).sum();
    assert!(seen > 0 && seen < 2000, "only the users seen, of 2,000: {seen}");
    let cross = c.post(&format!("/v1/live/{id}/cohorts"), json!({"by": ["age", "device"]})).await;
    assert!(cross["metrics"]["clicks"][0]["cells"].as_array().unwrap()[0]["cell"].as_str().unwrap().contains(" · "));
    let (s, _) = c.call(reqwest::Method::POST, &format!("/v1/live/{id}/cohorts"), Some(json!({"by": ["shoe size"]})), Some("t")).await;
    assert_eq!(s, 400);

    let list = c.get("/v1/experiments").await;
    assert_eq!(list["experiments"][0]["status"], "live");
    assert!(list["experiments"][0]["live"]["searches"].as_u64().unwrap() > 0);

    // Stopped: the run becomes an experiment with its report.
    c.post(&format!("/v1/live/{id}/stop"), json!({})).await;
    let r = c.done(&id).await;
    let rep = &r["report"];
    assert_eq!(rep["metrics"].as_array().unwrap().len(), 8);
    assert_eq!(rep["spec"]["mode"], "live");
    let searches = rep["searches"].as_u64().unwrap();
    assert!(searches >= 300, "{searches}");
    let srch = rep["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "success").unwrap();
    assert!(srch["comparisons"][0]["series"].as_array().unwrap().len() >= 2);
    assert!(rep["live"]["agent_tally"][0]["tasks"].is_number());
    // Why the agents gave up: per arm, counted, and summed up by the model
    // where some did.
    let why = &rep["live"]["why"];
    let gave: u64 = why["arms"].as_array().unwrap().iter().map(|a| a["gave_up"].as_u64().unwrap()).sum();
    if gave > 0 {
        assert_eq!(why["by"], "claude-opus-5-5", "{why}");
        let a = why["arms"].as_array().unwrap().iter().find(|a| a["gave_up"].as_u64().unwrap() > 0).unwrap();
        assert_eq!(a["text"], "the rows it showed were about something else", "{why}");
    } else {
        assert_eq!(why["by"], "counts", "{why}");
    }
    assert!(why["arms"].as_array().unwrap().iter().all(|a| a["text"].as_str().unwrap().len() > 10), "{why}");
    assert_eq!(rep["live"]["until"], "time");
    assert!(rep["live"]["decided"].is_null());
    assert_eq!(rep["live"]["visitor_tally"].as_array().unwrap().iter().map(|t| t["found"].as_u64().unwrap()).sum::<u64>(), 1);
    assert!(rep["cohorts"]["country"].is_object());
    assert_eq!(r["summary"]["mode"], "live");
    let after = c.get(&format!("/v1/live/{id}")).await;
    assert_eq!(after["status"], "done");

    // Every simulated search is in the rank log, and the clicks are events.
    let logged = server.query(&format!("SELECT COUNT(*) FROM exp_log_{id}"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert!(logged >= searches * 5, "a row per row shown: {logged} for {searches} searches");
    let events = server.query(&format!("SELECT COUNT(*) FROM exp_events_{id}"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert!(events > 0);
    let sat = server.query(&format!("SELECT COUNT(*) FROM exp_events_{id} WHERE action = 'satisfied' AND usr = 'you-tester'"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert_eq!(sat, 1, "the visitor's find");

    // Deleted with its log.
    let (s, _) = c.call(reqwest::Method::DELETE, &format!("/v1/experiments/{id}"), None, Some("t")).await;
    assert_eq!(s, 200);
    assert!(server.query(&format!("SELECT COUNT(*) FROM exp_log_{id}"), json!({}), None).await.is_err());
    let (s, _) = c.call(reqwest::Method::GET, &format!("/v1/live/{id}"), None, Some("t")).await;
    assert_eq!(s, 404);

    // A run that stops once decided: the body ranking against its own
    // matches shuffled, a loss no test misses, with two scripted agents.
    let shuffled = format!("(({}) * 7919) % 1", common::BODY);
    let spec = json!({"name": "decided", "site": "shop", "arms": [{"name": "Body", "rank": common::BODY}, {"name": "Shuffled", "rank": shuffled}], "users": 2000, "rate": 200, "minutes": 3, "agents": 2, "brain": "scripted"});
    let id = c.post("/v1/live", spec).await["id"].as_str().unwrap().to_string();
    let r = c.done(&id).await;
    let live = &r["report"]["live"];
    let d = &live["decided"];
    if d.is_object() {
        assert!(d["searches"].as_u64().unwrap() >= 1000 && d["elapsed"].as_f64().unwrap() < 180.0, "{d}");
        assert_eq!(d["verdicts"][0]["call"], "worse", "{d}");
        assert!(d["text"].as_str().unwrap().starts_with("Decided at "), "{d}");
        // It stopped there, not at its three minutes.
        assert!(live["elapsed"].as_f64().unwrap() < d["elapsed"].as_f64().unwrap() + 5.0, "{live}");
        assert!(live["waiting"].is_null(), "{live}");
    } else {
        // The call is made at once, but a guardrail can stay unread for the
        // three minutes: the query time is measured, so noisy on a busy
        // machine, and abandonment barely moves on this small site, so its
        // interval needs more searches than that to sit within its 1%. The
        // run says which it waits on.
        let w = live["waiting"].as_str().unwrap_or_default();
        assert!(w.starts_with("the guardrail "), "it decided, or says what it waits on: {live}");
        assert_eq!(r["report"]["verdicts"][0]["call"], "worse");
    }
    assert_eq!(live["why"]["by"], "counts");
    let agents = r["report"]["live"]["agent_tally"].as_array().unwrap().clone();
    assert!(agents.iter().all(|t| t["tasks"].as_u64().unwrap() >= 1), "scripted agents in both arms: {agents:?}");
}
