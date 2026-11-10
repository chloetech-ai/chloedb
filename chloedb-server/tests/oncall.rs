//! The on-call agent (`POST /v1/admin/oncall`): a stand-in for the Messages
//! API calls the read-only tools, then proposes actions, then reports; an
//! admin approves or rejects each proposal, and an approval is made with
//! the approver's key.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{extract::State, routing::post as route_post, Json as AxJson, Router};
use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Mock {
    seen: Arc<Mutex<Vec<Json>>>,
    /// An alert rule of the server's, which the script silences.
    rule: Arc<Mutex<String>>,
}

fn tool_use(id: &str, name: &str, input: Json) -> Json {
    json!({"type": "tool_use", "id": id, "name": name, "input": input})
}

/// The script: look (eight reads, one of them refused), then propose
/// (three proposals and one without a reason), then report. A question
/// that asks for a note is answered at once.
async fn messages(State(m): State<Mock>, AxJson(body): AxJson<Json>) -> AxJson<Json> {
    m.seen.lock().unwrap().push(body.clone());
    let turns = body["messages"].as_array().unwrap().len();
    let question = body["messages"][0]["content"].as_str().unwrap_or("");
    let rule = m.rule.lock().unwrap().clone();
    let usage = json!({"input_tokens": 1000, "output_tokens": 100});
    if question.contains("take your time") {
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    let content = match turns {
        _ if question.contains("note") => json!([{"type": "text", "text": "Summary: all is well."}]),
        1 => json!([
            {"type": "text", "text": "Starting broad."},
            tool_use("t1", "health", json!({})),
            tool_use("t2", "alerts", json!({})),
            tool_use("t3", "events", json!({"minutes": 30})),
            tool_use("t4", "queries", json!({"which": "slow", "limit": 5})),
            tool_use("t5", "timeseries", json!({"minutes": 5})),
            tool_use("t6", "logs", json!({"level": "warn"})),
            tool_use("t7", "contain_rules", json!({})),
            tool_use("t8", "alerts", json!({"rule": "../admin/keys"})),
        ]),
        3 => json!([
            tool_use("p1", "propose_contain", json!({"fingerprint": "f00dfeed", "action": "limit", "concurrency": 2, "expires_s": 600, "why": "one statement holds every core"})),
            tool_use("p2", "propose_silence_alert", json!({"rule": rule, "minutes": 15, "why": "a replica is being rebuilt"})),
            tool_use("p3", "propose_cancel_query", json!({"id": "987654", "why": "it has run for ten minutes"})),
            tool_use("p4", "propose_cancel_query", json!({"id": "1"})),
        ]),
        _ => json!([{"type": "text", "text": "Summary: one statement is slow.\nProposed actions: p1, p2, p3."}]),
    };
    let stop = if content.as_array().unwrap().iter().any(|b| b["type"] == "tool_use") { "tool_use" } else { "end_turn" };
    AxJson(json!({"content": content, "stop_reason": stop, "usage": usage}))
}

async fn send(base: &str, method: reqwest::Method, path: &str, key: &str, body: Option<Json>) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = c.request(method, format!("{base}{path}")).bearer_auth(key);
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Json::Null))
}

async fn post(base: &str, path: &str, key: &str, body: Json) -> (u16, Json) {
    send(base, reqwest::Method::POST, path, key, Some(body)).await
}

async fn get(base: &str, path: &str, key: &str) -> (u16, Json) {
    send(base, reqwest::Method::GET, path, key, None).await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_looks_proposes_and_an_admin_decides() {
    let mock = Mock::default();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = l.local_addr().unwrap();
    let app = Router::new().route("/v1/messages", route_post(messages)).with_state(mock.clone());
    tokio::spawn(async move { axum::serve(l, app).await });

    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), demo::schema(100), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    // Rows without their vectors, as a server writes them before its model opens.
    db.set_embed_later(true);
    db.load((0..100).map(|i| demo::product(i, 7))).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: Some("ak-oncall".into()), model: "claude-haiku-5-5".into(), base_url: format!("http://{addr}") };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(Arc::new(svc), l, Access { token: Some(Secret::new("root-token")), ..Default::default() }));
    let mut keys = Vec::new();
    for (name, scope) in [("reader", "read"), ("ops-ana", "admin"), ("ops-ben", "admin")] {
        let (status, j) = post(&base, "/v1/admin/keys", "root-token", json!({"name": name, "scope": scope})).await;
        assert_eq!(status, 200, "{j}");
        keys.push(j["secret"].as_str().unwrap().to_string());
    }
    let (read, ana, ben) = (&keys[0], &keys[1], &keys[2]);
    let (_, alerts) = get(&base, "/v1/alerts", ana).await;
    let rule = alerts["rules"][0]["id"].as_str().unwrap().to_string();
    *mock.rule.lock().unwrap() = rule.clone();

    // Only an admin runs it.
    let (status, _) = post(&base, "/v1/admin/oncall", read, json!({"question": "why is it slow?"})).await;
    assert_eq!(status, 403);
    let (status, j) = post(&base, "/v1/admin/oncall", ana, json!({})).await;
    assert_eq!(status, 400, "{j}");

    let (status, r) = post(&base, "/v1/admin/oncall", ana, json!({"alert": rule})).await;
    assert_eq!(status, 200, "{r}");
    assert!(r["answer"].as_str().unwrap().starts_with("Summary: one statement is slow."), "{r}");
    assert!(r["question"].as_str().unwrap().contains(&format!("Alert {rule} is firing")));
    let run = r["run"].as_str().unwrap().to_string();
    let steps = r["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 12, "{r}");
    let failed: Vec<&str> = steps.iter().filter(|s| s["ok"] == false).map(|s| s["tool"].as_str().unwrap()).collect();
    assert_eq!(failed, ["alerts", "propose_cancel_query"], "{r}");
    assert!(steps[7]["error"].as_str().unwrap().contains("not an id"));
    assert!(steps[11]["error"].as_str().unwrap().contains("why"));
    let proposals = r["proposals"].as_array().unwrap();
    let ids: Vec<&str> = proposals.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["p1", "p2", "p3"]);
    assert!(proposals.iter().all(|p| p["state"] == "pending" && p["run"] == run.as_str() && p["asked_by"].as_str().unwrap().contains("ops-ana")), "{r}");
    assert_eq!(proposals[0]["what"], "limit statement f00dfeed to 2 at once for 10 min");
    assert_eq!(r["usage"]["input_tokens"], 3000);

    {
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen[0]["system"].as_str().unwrap().contains("on-call engineer"));
        assert_eq!(seen[0]["tools"].as_array().unwrap().len(), 14);
        // Each read went to its route with the caller's key: the health
        // answer holds the rows, the slow queries their list.
        let results = seen[1]["messages"][2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 8);
        let text = |i: usize| results[i]["content"].as_str().unwrap().to_string();
        assert!(text(0).contains("\"docs\":100"), "{}", text(0));
        assert!(text(1).contains(&rule));
        assert!(text(3).contains("\"queries\""), "{}", text(3));
        assert!(results.iter().take(7).all(|r| r.get("is_error").is_none()), "{results:?}");
        assert_eq!(results[7]["is_error"], true);
    }

    // The proposals wait; an admin decides each once.
    let (_, list) = get(&base, "/v1/admin/oncall/proposals?state=pending", ben).await;
    assert_eq!(list["proposals"].as_array().unwrap().len(), 3, "{list}");
    let (status, _) = get(&base, "/v1/admin/oncall/proposals", read).await;
    assert_eq!(status, 403);
    let (status, _) = post(&base, "/v1/admin/oncall/proposals/p1/approve", read, json!({})).await;
    assert_eq!(status, 403);

    let (status, p) = post(&base, "/v1/admin/oncall/proposals/p1/approve", ben, json!({})).await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["state"], "applied", "{p}");
    assert!(p["decided_by"].as_str().unwrap().contains("ops-ben"), "{p}");
    let (_, rules) = get(&base, "/v1/admin/contain", ana).await;
    let held = rules.as_array().unwrap_or_else(|| panic!("{rules}"));
    assert_eq!(held.len(), 1, "{rules}");
    assert_eq!((held[0]["match"]["fingerprint"].as_str(), held[0]["concurrency"].as_u64(), held[0]["reason"].as_str()), (Some("f00dfeed"), Some(2), Some("one statement holds every core")));
    // The audit names the admin who approved it, with the proposal's reason.
    assert!(held[0]["by"].as_str().unwrap().contains("ops-ben"), "{}", held[0]);
    let (_, audit) = get(&base, "/v1/admin/audit?q=on-call%20proposal%20p1", ana).await;
    let entries = audit["entries"].as_array().unwrap_or_else(|| panic!("{audit}"));
    assert!(entries.iter().any(|e| e["route"] == "/v1/admin/contain" && e["who"].as_str().unwrap_or("").contains("ops-ben")), "{audit}");
    let (status, again) = post(&base, "/v1/admin/oncall/proposals/p1/approve", ana, json!({})).await;
    assert_eq!(status, 409, "{again}");

    let (status, p) = post(&base, "/v1/admin/oncall/proposals/p2/reject", ana, json!({"reason": "the rebuild is done"})).await;
    assert_eq!((status, p["state"].as_str()), (200, Some("rejected")), "{p}");
    let (_, a) = get(&base, &format!("/v1/alerts/{rule}"), ana).await;
    assert!(a.get("silenced").is_none() || a["silenced"].is_null(), "{a}");
    // A query that ended meanwhile: the approval fails, and says why.
    let (status, p) = post(&base, "/v1/admin/oncall/proposals/p3/approve", ana, json!({})).await;
    assert_eq!((status, p["state"].as_str()), (200, Some("failed")), "{p}");
    assert!(p["result"]["error"].as_str().unwrap().starts_with("404"), "{p}");
    let (status, _) = post(&base, "/v1/admin/oncall/proposals/p9/approve", ana, json!({})).await;
    assert_eq!(status, 404);

    // A note on the timeline.
    let (status, r) = post(&base, "/v1/admin/oncall", ana, json!({"question": "anything? leave a note", "note": true})).await;
    assert_eq!(status, 200, "{r}");
    assert!(r["note"].get("error").is_none(), "{r}");
    let (_, events) = get(&base, "/v1/events?kind=note", ana).await;
    let notes = events["events"].as_array().unwrap_or_else(|| panic!("{events}"));
    assert!(notes.iter().any(|e| e.to_string().contains("On-call agent") && e.to_string().contains("all is well")), "{events}");

    // The runs are kept, with what each tool answered and what the model
    // said between: the command center shows them.
    let (status, _) = get(&base, "/v1/admin/oncall", read).await;
    assert_eq!(status, 403);
    let (_, ov) = get(&base, "/v1/admin/oncall", ana).await;
    assert_eq!((ov["configured"].as_bool(), ov["model"].as_str(), ov["running"].as_u64()), (Some(true), Some("claude-haiku-5-5"), Some(0)), "{ov}");
    let runs = ov["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2, "{ov}");
    assert_eq!((runs[1]["id"].as_str(), runs[1]["summary"].as_str(), runs[1]["alert"].as_str()), (Some(run.as_str()), Some("one statement is slow."), Some(rule.as_str())), "{ov}");
    assert_eq!(runs[1]["proposals"], json!({"total": 3, "pending": 0}));
    let (_, one) = get(&base, &format!("/v1/admin/oncall/runs/{run}"), ben).await;
    assert_eq!((one["state"].as_str(), one["turns"].as_u64()), (Some("done"), Some(3)), "{one}");
    assert_eq!(one["said"], json!([{"before": 0, "turn": 1, "text": "Starting broad."}]));
    assert!(one["steps"][0]["preview"].as_str().unwrap().contains("\"docs\":100"), "{}", one["steps"][0]);
    assert!(one["steps"][1]["at_ms"].as_f64().unwrap() >= one["steps"][0]["at_ms"].as_f64().unwrap());
    assert_eq!(one["proposals"].as_array().unwrap().len(), 3);
    let (status, _) = get(&base, "/v1/admin/oncall/runs/oc1", ana).await;
    assert_eq!(status, 404);

    // A follow-up goes on in the run's conversation.
    let before = mock.seen.lock().unwrap().len();
    let (status, f) = post(&base, "/v1/admin/oncall", ben, json!({"question": "and what should I watch?", "follows": run})).await;
    assert_eq!(status, 200, "{f}");
    assert_eq!((f["follows"].as_str(), f["alert"].as_str(), f["state"].as_str()), (Some(run.as_str()), Some(rule.as_str()), Some("done")), "{f}");
    {
        let seen = mock.seen.lock().unwrap();
        let m = seen[before]["messages"].as_array().unwrap();
        assert_eq!(m.len(), 7, "the whole conversation, then the question");
        assert_eq!(m[1]["content"].as_array().unwrap().len(), 9, "its tool calls are in it");
        assert_eq!(m[6]["content"], "and what should I watch?");
    }
    let (status, j) = post(&base, "/v1/admin/oncall", ben, json!({"follows": run})).await;
    assert_eq!(status, 400, "{j}");

    // In the background: the answer is the run's id, and it can be stopped.
    let (status, bg) = post(&base, "/v1/admin/oncall", ana, json!({"question": "take your time", "wait": false})).await;
    assert_eq!((status, bg["state"].as_str()), (200, Some("running")), "{bg}");
    let id = bg["run"].as_str().unwrap().to_string();
    let (_, live) = get(&base, &format!("/v1/admin/oncall/runs/{id}"), ana).await;
    assert_eq!((live["state"].as_str(), live["doing"].as_str()), (Some("running"), Some("model")), "{live}");
    let (_, ov) = get(&base, "/v1/admin/oncall", ana).await;
    assert_eq!(ov["running"], 1);
    let (status, j) = post(&base, "/v1/admin/oncall", ben, json!({"question": "more?", "follows": id})).await;
    assert_eq!(status, 409, "a running run is followed up once it reports: {j}");
    let (status, s) = post(&base, &format!("/v1/admin/oncall/runs/{id}/stop"), ben, json!({})).await;
    assert_eq!(status, 200, "{s}");
    let t = std::time::Instant::now();
    let done = loop {
        let (_, r) = get(&base, &format!("/v1/admin/oncall/runs/{id}"), ana).await;
        if r["state"] != "running" {
            break r;
        }
        assert!(t.elapsed() < Duration::from_secs(4), "a stop drops the model call under way: {r}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(done["state"], "stopped", "{done}");
    assert!(done["stopped_by"].as_str().unwrap().contains("ops-ben"), "{done}");
    assert!(done["answer"].as_str().unwrap().starts_with("Stopped by"), "{done}");
    let (status, _) = post(&base, &format!("/v1/admin/oncall/runs/{id}/stop"), ben, json!({})).await;
    assert_eq!(status, 409);
    let (_, list) = get(&base, "/v1/admin/oncall/runs", ana).await;
    assert_eq!(list["runs"].as_array().unwrap().len(), 4, "{list}");
}
