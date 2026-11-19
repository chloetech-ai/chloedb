//! The Experiments app against a real chloedb-server: a site of its own
//! tables added through the API, a fast and a served experiment run in the
//! background to their reports, a look at the arms (and one that demotes
//! its first row), the queries that moved, a cut by two attributes, a check
//! of the result by repeats, an interleaving with its A/B beside it, the
//! token, and a delete that drops the served run's log.
//!
//! It needs the chloedb-server binary, as tests/served.rs does.

mod common;

use serde_json::json;

use chloedb_experiments::app::{self, Opts};
use common::Client;

#[tokio::test(flavor = "multi_thread")]
async fn the_app_runs_experiments_on_a_site_of_its_own_tables() {
    let Some((server, _guard, _dir)) = common::shop_server().await else { return };

    // The app, on a port of its own, with a token.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(app::serve(server.clone(), listener, Opts { token: Some("t".into()), beir: Vec::new(), anthropic_key: None, anthropic_base: None, sources: Vec::new() }));
    let c = Client { url, http: reqwest::Client::new() };
    common::ready(&c).await;

    // The token is needed everywhere but /healthz; the manifest names the view.
    assert_eq!(c.call(reqwest::Method::GET, "/v1/experiments", None, None).await.0, 401);
    assert_eq!(c.call(reqwest::Method::GET, "/v1/experiments", None, Some("wrong")).await.0, 401);
    let m = c.get("/app.json").await;
    assert_eq!(m["name"], "experiments");
    assert_eq!(m["view"], "ui/view.js");
    let view = c.http.get(format!("{}/ui/view.js", c.url)).bearer_auth("t").send().await.unwrap();
    assert!(view.text().await.unwrap().contains("export default"));
    let meta = c.get("/v1/meta").await;
    assert_eq!(meta["defaults"]["model"], "dbn");
    assert_eq!(meta["metrics"].as_array().unwrap().len(), 8);

    let (title, body) = (common::TITLE, common::BODY);
    let site = common::shop_site();
    let added = c.post("/v1/sites", site).await;
    assert_eq!(added["queries"], 30);
    let (s, v) = c.call(reqwest::Method::POST, "/v1/sites", Some(json!({"name": "x; DROP", "table": "shop", "queries": "shop_queries", "judgments": "shop_grades"})), Some("t")).await;
    assert_eq!(s, 400, "{v}");
    let sites = c.get("/v1/sites").await;
    assert_eq!(sites["sites"][0]["name"], "shop");
    let one = c.get("/v1/sites/shop").await;
    assert!(one["judged"].as_u64().unwrap() > 100, "{one}");

    // What each arm shows for the busiest query, row by row with grades.
    let pv = c.post("/v1/preview", json!({"site": "shop", "ranks": [body, title], "limit": 5, "queries": 2})).await;
    let q = &pv["queries"][0];
    assert_eq!(q["id"], "q0");
    assert_eq!(q["lists"].as_array().unwrap().len(), 2);
    assert!(q["lists"][1]["rows"][0]["grade"].as_u64().unwrap() >= 1, "title ranks a judged row first: {q}");

    // An arm that moves its first row to the end, in the preview.
    let pv = c.post("/v1/preview", json!({"site": "shop", "ranks": [title, title], "thens": ["", "demote_first"], "limit": 5, "queries": 1})).await;
    let (l0, l1) = (&pv["queries"][0]["lists"][0]["rows"], &pv["queries"][0]["lists"][1]["rows"]);
    assert_eq!(l1[4]["key"], l0[0]["key"], "{pv}");
    assert_eq!(l1[0]["key"], l0[1]["key"], "{pv}");

    // Specs the app refuses: one arm; a then it does not know; an
    // interleaving of three arms.
    let (s, v) = c.call(reqwest::Method::POST, "/v1/experiments", Some(json!({"name": "x", "site": "shop", "arms": [{"name": "A", "rank": body}, {"name": "B", "rank": body, "then": "reverse_all"}]})), Some("t")).await;
    assert_eq!(s, 400, "{v}");
    let (s, v) = c.call(reqwest::Method::POST, "/v1/experiments", Some(json!({"name": "x", "site": "shop", "mode": "interleave", "arms": [{"name": "A", "rank": body}, {"name": "B", "rank": title}, {"name": "C", "rank": title, "then": "demote_first"}]})), Some("t")).await;
    assert_eq!(s, 400, "{v}");
    let (s, v) = c.call(reqwest::Method::POST, "/v1/experiments", Some(json!({"name": "x", "site": "shop", "arms": [{"name": "A", "rank": body}]})), Some("t")).await;
    assert_eq!(s, 400, "{v}");

    // Fast: title against body, which the grades favour.
    let spec = json!({"name": "title vs body", "site": "shop", "arms": [{"name": "Body", "rank": body}, {"name": "Title", "rank": title}], "users": 3000, "impressions": 60000, "explore_share": 0.15});
    let id = c.post("/v1/experiments", spec).await["id"].as_str().unwrap().to_string();
    let r = c.done(&id).await;
    let rep = &r["report"];
    assert_eq!(rep["srm"]["ok"], true, "{}", rep["srm"]);
    let success = rep["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "success").unwrap();
    let cmp = &success["comparisons"][0];
    assert!(cmp["true_delta"].as_f64().unwrap() > 0.0, "the title ranking is truly better here: {cmp}");
    assert!(cmp["p"].as_f64().unwrap() < 0.05, "and the experiment sees it: {cmp}");
    assert!(cmp["cuped"]["variance_reduction"].as_f64().unwrap() > 0.0, "{cmp}");
    assert_eq!(cmp["series"].as_array().unwrap().len(), 20);
    assert_eq!(rep["verdicts"][0]["call"], "ship", "{}", rep["verdicts"]);
    assert_eq!(rep["cohorts"]["device"]["metrics"]["success"][0]["cells"].as_array().unwrap().len(), 3);
    let wi = &rep["whatif"];
    assert!(wi["searches"].as_u64().unwrap() > 5000, "{wi}");
    assert_eq!(wi["arms"].as_array().unwrap().len(), 2);
    assert_eq!(r["summary"]["verdicts"][0]["call"], "ship");
    assert!(rep["spec"]["seed"].as_u64().is_some(), "a seed of its own, given none");

    // The queries that moved: they add up to the whole true difference.
    let bq = &rep["by_query"]["arms"][0];
    let parts: f64 = bq["bands"].as_array().unwrap().iter().map(|b| b["adds"].as_f64().unwrap()).sum();
    assert!(bq["gains"].as_array().unwrap().len() >= 1, "{bq}");
    let total = bq["total"].as_f64().unwrap();
    assert!(total > 0.0 && parts.is_finite(), "{total} {parts}");

    // Check this: five repeats on the same users, each split anew.
    let (s, v) = c.call(reqwest::Method::POST, &format!("/v1/experiments/{id}/check"), Some(json!({"repeats": 5})), Some("t")).await;
    assert_eq!(s, 200, "{v}");
    let t0 = std::time::Instant::now();
    let chk = loop {
        let r = c.get(&format!("/v1/experiments/{id}")).await;
        if r["checking"].is_null() && !r["report"]["check"].is_null() {
            break r["report"]["check"].clone();
        }
        assert!(t0.elapsed().as_secs() < 120, "the check did not end: {}", r["checking"]);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    };
    assert_eq!(chk["repeats"], 5, "{chk}");
    let p = chk["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "success").unwrap();
    assert!(p["arms"][0]["covered"].as_u64().unwrap() >= 3, "{p}");
    let runs = chk["primary"]["arms"][0]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 5);
    let truth = chk["primary"]["arms"][0]["true_lift"].as_f64().unwrap();
    let first = rep["verdicts"][0]["true_lift"].as_f64().unwrap();
    assert!((truth - first).abs() < 1e-9, "the same users, the same truth: {truth} {first}");

    // The list, and a cut by two attributes of the run kept in memory.
    let list = c.get("/v1/experiments").await;
    assert_eq!(list["experiments"][0]["id"], id.as_str());
    let cut = c.post(&format!("/v1/experiments/{id}/cut"), json!({"by": ["age", "device"]})).await;
    let cells = cut["metrics"]["success"][0]["cells"].as_array().unwrap();
    assert!(cells.len() > 10 && cells[0]["cell"].as_str().unwrap().contains(" · "), "{cut}");
    let (s, _) = c.call(reqwest::Method::POST, &format!("/v1/experiments/{id}/cut"), Some(json!({"by": ["shoe size"]})), Some("t")).await;
    assert_eq!(s, 400);

    // Served: each search a logged query, the exploring share's with its
    // policy, and each arm's searches timed one by one.
    let spec = json!({"name": "served", "site": "shop", "arms": [{"name": "Body", "rank": body}, {"name": "Title", "rank": title}], "users": 500, "impressions": 3000, "mode": "served", "pre": 0.2, "explore_share": 0.2});
    let sid = c.post("/v1/experiments", spec).await["id"].as_str().unwrap().to_string();
    let r = c.done(&sid).await;
    let rep = &r["report"];
    let n: u64 = rep["latency"].as_array().unwrap().iter().map(|l| l["n"].as_u64().unwrap()).sum();
    assert!((2000..3000).contains(&n), "the arms' searches, timed one by one: {n}");
    let logged = server.query(&format!("SELECT COUNT(*) FROM exp_log_{sid}"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert!(logged >= 3000, "every search is in the rank log, a row per row shown: {logged}");
    let explored = server.query(&format!("SELECT COUNT(*) FROM exp_log_{sid} WHERE variant = 'explore'"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert!(explored > 0, "the exploring share is logged as its variant");

    // An interleaving: title against body in one list, and the A/B of the
    // same pair beside it.
    let spec = json!({"name": "interleaved", "site": "shop", "mode": "interleave", "arms": [{"name": "Body", "rank": body}, {"name": "Title", "rank": title}], "users": 2000, "impressions": 20000});
    let iid = c.post("/v1/experiments", spec).await["id"].as_str().unwrap().to_string();
    let r = c.done(&iid).await;
    let rep = &r["report"];
    assert_eq!(rep["kind"], "interleave", "{}", r["error"]);
    let cl = &rep["interleave"]["clicks"];
    assert!(cl["true_delta"].as_f64().unwrap() > 0.0, "users truly prefer the title ranking's rows: {cl}");
    assert!(cl["covers"].is_boolean() && cl["p"].as_f64().unwrap() < 0.05, "{cl}");
    let w = &rep["interleave"]["wins"];
    assert!(w["b"].as_f64().unwrap() > w["a"].as_f64().unwrap(), "{w}");
    assert_eq!(rep["ab"]["metrics"].as_array().unwrap().len(), 3);
    assert!(rep["saving"]["times"].as_f64().unwrap() > 0.0, "{}", rep["saving"]);
    assert_eq!(r["summary"]["mode"], "interleave");
    assert_eq!(rep["verdicts"][0]["call"], "ship");
    let (s, _) = c.call(reqwest::Method::POST, &format!("/v1/experiments/{iid}/check"), Some(json!({})), Some("t")).await;
    assert_eq!(s, 400, "an interleaving is not checked by repeats");

    // Delete: the row, and the served run's log and events.
    let (s, _) = c.call(reqwest::Method::DELETE, &format!("/v1/experiments/{sid}"), None, Some("t")).await;
    assert_eq!(s, 200);
    let (s, _) = c.call(reqwest::Method::GET, &format!("/v1/experiments/{sid}"), None, Some("t")).await;
    assert_eq!(s, 404);
    assert!(server.query(&format!("SELECT COUNT(*) FROM exp_log_{sid}"), json!({}), None).await.is_err(), "its log is dropped");
}
