//! Studies of a real site, against a made-up one: a small site that speaks
//! the experiment protocol (as chloedb-hn and chloedb-arxiv do) with one
//! knob, `broken`, that ranks the rows worst first. The Experiments app
//! (on a real chloedb-server for its own tables) lists it with its knobs,
//! refuses a site that will not take experiments, searches it with knobs,
//! writes personas from its rows, and runs a paired study of the site as
//! it is against the broken arm, scripted (no model), to a report that
//! says the broken arm finds less, need by need.
//!
//! It needs the chloedb-server binary, as tests/app.rs does.

mod common;

use std::time::{Duration, Instant};

use axum::extract::{Path, Query};
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
                "knobs": [{"key": "broken", "group": "Ranking", "title": "Worst first", "kind": "bool", "default": false}],
                "presets": [{"name": "As it is", "knobs": {}}, {"name": "Broken", "knobs": {"broken": true}}]})))
        }))
        .route("/v1/experiment/search", post(|h: HeaderMap, Json(b): Json<Value>| async move {
            authed(&h)?;
            if b["knobs"].as_object().is_some_and(|o| o.keys().any(|k| k != "broken")) {
                return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "not a knob of this site"}))));
            }
            let q = words(b["q"].as_str().unwrap_or(""));
            let mut scored: Vec<(usize, usize)> = (0..ROWS.len()).map(|i| (words(ROWS[i]).iter().filter(|w| q.contains(w)).count(), i)).collect();
            scored.sort_by_key(|&(s, i)| (std::cmp::Reverse(s), i));
            if b["knobs"]["broken"] == true {
                scored.reverse();
            }
            let per = b["per"].as_u64().unwrap_or(10) as usize;
            let rows: Vec<Value> = scored.iter().filter(|(s, _)| b["knobs"]["broken"] == true || *s > 0).take(per).map(|&(_, i)| row(i)).collect();
            Ok(Json(json!({"q": b["q"], "rows": rows, "understood": []})))
        }))
        .route("/v1/experiment/item/:id", get(|h: HeaderMap, Path(id): Path<String>| async move {
            authed(&h)?;
            let i: usize = id.trim_start_matches('s').parse::<usize>().unwrap_or(1).saturating_sub(1).min(ROWS.len() - 1);
            Ok::<_, (StatusCode, Json<Value>)>(Json(json!({"id": id, "title": ROWS[i], "url": "", "text": format!("{}\n\nThe whole story.", ROWS[i])})))
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let sources = vec![Source::new("made-up", Kind::App, &site_url, Some(TOKEN.into())), Source::new("closed", Kind::App, &site_url, Some("wrong".into()))];
    tokio::spawn(app::serve(server.clone(), listener, Opts { token: Some("t".into()), beir: Vec::new(), anthropic_key: None, anthropic_base: None, sources }));
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
}
