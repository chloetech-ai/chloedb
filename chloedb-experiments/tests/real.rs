//! Studies of a real site, against a made-up one: a small site that speaks
//! the experiment protocol (as chloedb-hn and chloedb-arxiv do) with one
//! knob, `broken`, that ranks the rows worst first. The Experiments app
//! (on a real chloedb-server for its own tables) lists it with its knobs,
//! refuses a site that will not take experiments, searches it with knobs,
//! writes personas from its rows, and runs a paired study of the site as
//! it is against the broken arm, scripted (no model), to a report that
//! says the broken arm finds less, need by need.
//!
//! The same study is then run by Claude, against a stand-in of the
//! Messages API that refuses a forced tool choice as the newest models do:
//! it writes the people, plays them on each arm, and grades what a topical
//! need was shown.
//!
//! It needs the chloedb-server binary, as tests/app.rs does.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use chloedb_experiments::app::{self, Opts};
use chloedb_experiments::source::{Kind, Source};
use common::Client;

const ROWS: [&str; 12] = [
    "Rust compiler speeds builds twofold",
    "Postgres replication arrives with version sixteen",
    "Bitcoin passes thousand dollars again",
    "Apple announces phone without keyboard",
    "Kubernetes operator backs databases nightly",
    "Remote workers prefer quiet offices",
    "Climate models predict wetter winters",
    "Linux kernel accepts drivers written safely",
    "Startup founders regret raising early",
    "Privacy regulators fine advertising giant",
    "Space telescope finds distant galaxies",
    "Javascript framework fatigue explained honestly",
];

const TOKEN: &str = "site-token";

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 3).map(|w| w.to_lowercase()).collect()
}

fn row(i: usize) -> Value {
    json!({"id": format!("s{}", i + 1), "title": ROWS[i], "snippet": format!("{} points · by someone", 100 + i), "url": format!("https://example.com/{i}")})
}

fn authed(h: &HeaderMap) -> Result<(), (StatusCode, Json<Value>)> {
    match h.get("authorization").and_then(|v| v.to_str().ok()) {
        Some(v) if v == format!("Bearer {TOKEN}") => Ok(()),
        _ => Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "experiments need the site's experiments token"})))),
    }
}

async fn site() -> String {
    let app = Router::new()
        .route("/v1/experiment", get(|h: HeaderMap| async move {
            authed(&h)?;
            Ok::<_, (StatusCode, Json<Value>)>(Json(json!({"protocol": 1, "site": "made-up", "title": "A made-up news site", "about": "Stories of a made-up news site.", "rows": "stories",
                "knobs": [{"key": "broken", "group": "Ranking", "title": "Worst first", "kind": "bool", "default": false}, {"key": "noise", "group": "Ranking", "title": "Noise", "kind": "number", "default": 0.0, "min": 0.0, "max": 3.0, "step": 0.05}],
                "presets": [{"name": "As it is", "knobs": {}}, {"name": "Broken", "knobs": {"broken": true}}],
                "log": {"policy": {"policy": "epsilon", "epsilon": 0.5, "top": 4}, "path": "/v1/experiment/log"}})))
        }))
        .route("/v1/experiment/search", post(|h: HeaderMap, Json(b): Json<Value>| async move {
            authed(&h)?;
            if b["knobs"].as_object().is_some_and(|o| o.keys().any(|k| k != "broken" && k != "noise")) {
                return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "not a knob of this site"}))));
            }
            let q = words(b["q"].as_str().unwrap_or(""));
            let mut scored: Vec<(usize, usize)> = (0..ROWS.len()).map(|i| (words(ROWS[i]).iter().filter(|w| q.contains(w)).count(), i)).collect();
            scored.sort_by_key(|&(s, i)| (std::cmp::Reverse(s), i));
            // Noise: each row's matches less noise × a fixed jitter of it (0 to 2),
            // and rows that match nothing are shown too.
            let noise = b["knobs"]["noise"].as_f64().unwrap_or(0.0);
            let jitter = |i: usize| ((i * 7919) % 13) as f64 / 13.0 * 2.0;
            scored.sort_by(|a, b| (b.0 as f64 - noise * jitter(b.1)).total_cmp(&(a.0 as f64 - noise * jitter(a.1))).then(a.1.cmp(&b.1)));
            if b["knobs"]["broken"] == true {
                scored.reverse();
            }
            let per = b["per"].as_u64().unwrap_or(10) as usize;
            let rows: Vec<Value> = scored.iter().filter(|(s, _)| b["knobs"]["broken"] == true || noise > 0.0 || *s > 0).take(per).map(|&(_, i)| row(i)).collect();
            Ok(Json(json!({"q": b["q"], "rows": rows, "understood": []})))
        }))
        .route("/v1/experiment/item/:id", get(|h: HeaderMap, Path(id): Path<String>| async move {
            authed(&h)?;
            let i: usize = id.trim_start_matches('s').parse::<usize>().unwrap_or(1).saturating_sub(1).min(ROWS.len() - 1);
            Ok::<_, (StatusCode, Json<Value>)>(Json(json!({"id": id, "title": ROWS[i], "url": "", "text": format!("{}\n\nThe whole story.", ROWS[i])})))
        }))
        .route("/v1/experiment/log", get(|h: HeaderMap| async move {
            authed(&h)?;
            // 40 searches of the page, half exploring, the first story opened each time.
            let reqs: Vec<Value> = (0..40).map(|i| {
                let shown = if i % 2 == 0 { json!(["s1", "s2", "s3", "s4"]) } else { json!(["s3", "s1", "s4", "s2"]) };
                json!({"request": format!("r{i}"), "ts": 1, "q": "rust compiler", "explored": i % 2 == 1, "ranked": ["s1", "s2", "s3", "s4"], "shown": shown, "clicks": ["s1"]})
            }).collect();
            Ok::<_, (StatusCode, Json<Value>)>(Json(json!({"policy": {"policy": "epsilon", "epsilon": 0.5, "top": 4}, "requests": reqs, "clicks": 40})))
        }))
        .route("/v1/experiment/sample", get(|h: HeaderMap, Query(_q): Query<Value>| async move {
            authed(&h)?;
            Ok::<_, (StatusCode, Json<Value>)>(Json(json!({"rows": (0..ROWS.len()).map(row).collect::<Vec<_>>()})))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

type Seen = Arc<Mutex<Vec<Value>>>;

/// A stand-in of the Messages API. It refuses a forced tool choice, as
/// Claude Opus 5.5 and Sonnet 5.5 do; it writes two people (each
/// remembering a row, and wanting a topic), grades the rows about
/// telescopes 2 and the rest 0, and plays a person: the need's words
/// searched, the first result opened and named.
async fn messages(State(seen): State<Seen>, Json(b): Json<Value>) -> (StatusCode, Json<Value>) {
    seen.lock().unwrap().push(b.clone());
    if b["tool_choice"]["type"] != "auto" {
        return (StatusCode::BAD_REQUEST, Json(json!({"type": "error", "error": {"type": "invalid_request_error", "message": "tool_choice: type \"tool\" and \"any\" are not supported for this model."}})));
    }
    let tool = b["tools"][0]["name"].as_str().unwrap_or("").to_string();
    let msgs = b["messages"].as_array().cloned().unwrap_or_default();
    let first = msgs[0]["content"].as_str().unwrap_or("").to_string();
    let call = match tool.as_str() {
        "personas" => json!({"name": "personas", "input": {"personas": [
            {"name": "Ana, a compiler engineer", "about": "Builds compilers.", "needs": [{"kind": "known", "need": "rust compiler builds faster", "row": 1}, {"kind": "topical", "need": "space telescope galaxies", "row": 0}]},
            {"name": "Ben, a sysadmin", "about": "Runs Linux machines.", "needs": [{"kind": "known", "need": "linux kernel drivers safely", "row": 8}, {"kind": "topical", "need": "telescope finds galaxies", "row": 0}]}
        ]}}),
        "grades" => {
            let grades: Vec<Value> = first
                .lines()
                .filter_map(|l| {
                    let (n, t) = l.split_once(". ")?;
                    let n: u64 = n.trim().parse().ok()?;
                    Some(json!({"n": n, "grade": if t.to_lowercase().contains("telescope") { 2 } else { 0 }}))
                })
                .collect();
            json!({"name": "grades", "input": {"grades": grades}})
        }
        _ => {
            let need = first.split("looking for this:\n\n\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
            match msgs.iter().filter(|m| m["role"] == "assistant").count() {
                0 => json!({"name": "search", "input": {"query": need, "why": "my words"}}),
                1 => json!({"name": "open", "input": {"result": 1, "why": "the first looks right"}}),
                _ => json!({"name": "done", "input": {"found": true, "result": 1, "note": "that was it"}}),
            }
        }
    };
    let turn = msgs.len();
    (
        StatusCode::OK,
        Json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": b["model"], "stop_reason": "tool_use",
            "content": [{"type": "thinking", "thinking": "", "signature": "sig"}, {"type": "tool_use", "id": format!("toolu_{turn}"), "name": call["name"], "input": call["input"]}],
            "usage": {"input_tokens": 1000, "output_tokens": 200},
        })),
    )
}

async fn until(c: &Client, path: &str, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let t0 = Instant::now();
    loop {
        let v = c.get(path).await;
        if ok(&v) {
            return v;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "{what}: {v}");
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_study_of_a_real_site_finds_the_broken_arm() {
    let Some((server, _guard, _dir)) = common::shop_server().await else { return };
    let site_url = site().await;
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let fake = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", fake.local_addr().unwrap());
    let r = Router::new().route("/v1/messages", post(messages)).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(fake, r).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let sources = vec![Source::new("made-up", Kind::App, &site_url, Some(TOKEN.into())), Source::new("closed", Kind::App, &site_url, Some("wrong".into()))];
    tokio::spawn(app::serve(server.clone(), listener, Opts { token: Some("t".into()), beir: Vec::new(), anthropic_key: Some("k".into()), anthropic_base: Some(base), sources }));
    let c = Client { url, http: reqwest::Client::new() };
    common::ready(&c).await;

    // The sites, with their knobs; one refuses the token it was given.
    let real = c.get("/v1/real").await;
    let s = &real["sources"];
    assert_eq!((s[0]["source"].clone(), s[0]["allowed"].clone(), s[0]["knobs"][0]["key"].clone()), (json!("made-up"), json!(true), json!("broken")), "{real}");
    assert_eq!(s[1]["allowed"], false, "{real}");
    assert!(s[1]["error"].as_str().unwrap().contains("401"), "{real}");
    assert_eq!(c.get("/v1/meta").await["real"]["sources"], json!(["made-up", "closed"]));

    // A search of it, as each arm sees it.
    let p = c.post("/v1/real/made-up/search", json!({"q": "rust compiler", "knobs": {}})).await;
    assert_eq!(p["rows"][0]["title"], ROWS[0], "{p}");
    let p = c.post("/v1/real/made-up/search", json!({"q": "rust compiler", "knobs": {"broken": true}})).await;
    assert_ne!(p["rows"][0]["title"], ROWS[0], "{p}");
    let (st, _) = c.call(reqwest::Method::POST, "/v1/real/made-up/search", Some(json!({"q": "x", "knobs": {"nope": 1}})), Some("t")).await;
    assert_eq!(st, 502);
    let it = c.get("/v1/real/made-up/item/s2").await;
    assert!(it["text"].as_str().unwrap().contains("The whole story"), "{it}");

    // Personas from its rows, by the stand-in; none from the closed site.
    let (st, v) = c.call(reqwest::Method::POST, "/v1/personas", Some(json!({"source": "closed", "brain": "scripted"})), Some("t")).await;
    assert_eq!(st, 400, "{v}");
    let w = c.post("/v1/personas", json!({"source": "made-up", "brain": "scripted", "count": 3, "needs": 2, "seed": 3})).await;
    let id = w["id"].as_str().unwrap().to_string();
    let set = until(&c, &format!("/v1/personas/{id}"), "the personas", |v| v["status"] == "ready" || v["status"] == "failed").await;
    assert_eq!(set["status"], "ready", "{set}");
    assert_eq!(set["personas"].as_array().unwrap().len(), 3);
    let needs: Vec<&Value> = set["personas"].as_array().unwrap().iter().flat_map(|p| p["needs"].as_array().unwrap()).collect();
    assert_eq!(needs.len(), 6);
    assert!(needs.iter().filter(|n| n["kind"] == "known").all(|n| n["target"].as_str().is_some_and(|t| t.starts_with('s'))), "{set}");
    assert_eq!(c.get("/v1/personas").await["sets"][0]["personas"], 3);

    // A study: the site as it is against the broken arm; one with a closed arm is refused.
    let arms = json!([{"name": "As it is", "source": "made-up", "knobs": {}}, {"name": "Broken", "source": "made-up", "knobs": {"broken": true}}]);
    let (st, v) = c.call(reqwest::Method::POST, "/v1/studies", Some(json!({"name": "x", "personas": id, "arms": [arms[0], {"name": "Closed", "source": "closed"}], "brain": "scripted"})), Some("t")).await;
    assert_eq!(st, 400, "{v}");
    let (st, v) = c.call(reqwest::Method::POST, "/v1/studies", Some(json!({"name": "x", "personas": id, "arms": [arms[0], {"name": "Bad knob", "source": "made-up", "knobs": {"nope": 1}}], "brain": "scripted"})), Some("t")).await;
    assert_eq!(st, 400, "a knob the site refuses: {v}");
    let s = c.post("/v1/studies", json!({"name": "Broken ranking", "personas": id, "arms": arms, "brain": "scripted", "budget": 1, "workers": 2})).await;
    let sid = s["id"].as_str().unwrap().to_string();
    let v = until(&c, &format!("/v1/studies/{sid}"), "the study", |v| v["status"] != "running").await;
    assert_eq!((v["status"].clone(), v["done"].clone(), v["total"].clone()), (json!("done"), json!(12), json!(12)), "{v}");
    let r = &v["report"];
    let (a, b) = (r["arms"][0]["known_success"]["k"].as_u64().unwrap(), r["arms"][1]["known_success"]["k"].as_u64().unwrap());
    assert_eq!((a, b), (3, 0), "the known stories, found as it is and not when broken: {r}");
    let cmp = &r["comparisons"][0];
    assert_eq!((cmp["pairs"].clone(), cmp["only_control"].clone()), (json!(6), json!(3)), "{cmp}");
    assert!(r["arms"][0]["ndcg"]["mean"].as_f64().unwrap() > r["arms"][1]["ndcg"]["mean"].as_f64().unwrap(), "{r}");
    assert!(r["arms"][0]["known_on_first_page"]["mean"] == 1.0, "{r}");
    // Each need, as each arm did, and its sessions step by step.
    let n0 = r["needs"][0]["id"].as_str().unwrap();
    let ss = c.get(&format!("/v1/studies/{sid}/sessions?need={n0}")).await;
    let ss = ss["sessions"].as_array().unwrap();
    assert_eq!(ss.len(), 2);
    assert!(ss.iter().all(|s| s["steps"][0]["kind"] == "search"), "{ss:?}");
    // Kept: the list shows it, and its view outlives the memory of it.
    let list = c.get("/v1/studies").await;
    assert_eq!(list["studies"][0]["id"], json!(sid));
    assert_eq!(list["studies"][0]["status"], "done");
    let t0 = Instant::now();
    loop {
        let a = server.query(&format!("SELECT status FROM exp_studies WHERE id = '{sid}'"), json!({}), None).await.unwrap();
        if a["rows"][0][0] == "done" {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "the study is kept: {a}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // A tune: from a noisy base, the search lowers the noise on the study's
    // needs and grades, and proves it on the needs held out.
    let (st, v) = c.call(reqwest::Method::POST, "/v1/tunes", Some(json!({"study": sid, "source": "closed", "brain": "scripted"})), Some("t")).await;
    assert_eq!(st, 400, "a site that takes no experiments: {v}");
    let t = c.post("/v1/tunes", json!({"study": sid, "source": "made-up", "base": {"noise": 2.0}, "brain": "scripted", "rounds": 3})).await;
    let tid = t["id"].as_str().unwrap().to_string();
    let v = until(&c, &format!("/v1/tunes/{tid}"), "the tune", |v| v["status"] != "running").await;
    assert_eq!(v["status"], "done", "{v}");
    let r = &v["report"];
    assert_eq!(v["needs"], 6, "every need searched in the study: {v}");
    assert!(v["held_out"].as_u64().unwrap() >= 1, "{v}");
    let ch = r["changed"].as_array().unwrap();
    assert!(ch.len() == 1 && ch[0]["key"] == "noise" && ch[0]["to"].as_f64().unwrap() < 2.0, "less noise: {r}");
    assert!(r["train"]["after"].as_f64().unwrap() > r["train"]["before"].as_f64().unwrap(), "{r}");
    assert_eq!(r["proposal"]["noise"], ch[0]["to"], "{r}");
    assert!(r["held_out"]["verdict"].is_string() && r["held_out"]["sentence"].is_string(), "{r}");
    assert!(v["searches"].as_u64().unwrap() > 0 && v["steps"].as_array().unwrap().len() > 2, "{v}");
    assert_eq!(c.get("/v1/tunes").await["tunes"][0]["id"], json!(tid));
    // The site logs its page's searches: both rankings valued on them.
    let op = &r["off_policy"];
    assert_eq!((op["requests"].clone(), op["explored"].clone(), op["clicks"].clone()), (json!(40), json!(20), json!(40)), "{op}");
    let t = op["targets"].as_array().unwrap();
    assert_eq!(t.iter().map(|t| t["name"].as_str().unwrap()).collect::<Vec<_>>(), ["as logged", "as it is", "proposed"]);
    let naive = t[0]["estimates"].as_array().unwrap().iter().find(|e| e["estimator"] == "naive").unwrap();
    assert_eq!(naive["value"], 1.0, "one story opened a search: {op}");

    // The same, by Claude: the people written, played and judged.
    let w = c.post("/v1/personas", json!({"source": "made-up", "brain": "claude"})).await;
    let cid = w["id"].as_str().unwrap().to_string();
    let set = until(&c, &format!("/v1/personas/{cid}"), "Claude's personas", |v| v["status"] == "ready" || v["status"] == "failed").await;
    assert_eq!(set["status"], "ready", "{set}");
    assert_eq!(set["personas"][0]["needs"][0]["target"], "s1", "{set}");
    assert!(set["cost"].as_f64().unwrap() > 0.0, "{set}");
    let s = c.post("/v1/studies", json!({"name": "Broken, by Claude", "personas": cid, "arms": arms, "brain": "claude", "agent_model": "claude-haiku-5-5", "judge_model": "claude-haiku-5-5", "budget": 1, "workers": 2})).await;
    let sid = s["id"].as_str().unwrap().to_string();
    let v = until(&c, &format!("/v1/studies/{sid}"), "Claude's study", |v| v["status"] != "running").await;
    assert_eq!((v["status"].clone(), v["done"].clone(), v["error"].clone()), (json!("done"), json!(8), Value::Null), "{v}");
    let r = &v["report"];
    let k = |a: usize, m: &str| r["arms"][a][m]["k"].as_u64().unwrap();
    assert_eq!((k(0, "known_success"), k(1, "known_success")), (2, 0), "{r}");
    assert_eq!(k(0, "topical_success"), 2, "the judge's 2 for the telescope: {r}");
    assert!(v["judge_spent"].as_f64().unwrap() > 0.0 && v["spent"].as_f64().unwrap() > 0.0, "{v}");
    let calls = seen.lock().unwrap().clone();
    assert!(calls.iter().all(|b| b["tool_choice"]["type"] == "auto"), "no forced tool choice");
    assert!(calls.iter().any(|b| b["tools"][0]["name"] == "grades"), "the judge was asked");
}
