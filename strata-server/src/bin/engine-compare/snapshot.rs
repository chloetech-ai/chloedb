//! Every engine's request for every kind, for one fixed query, compared
//! with `testdata/requests.json`: a refactor of the requests (such as
//! filters, prefixes or depths that vary by request) must leave today's
//! requests byte for byte as they were. `ENGINE_COMPARE_SNAPSHOT=update`
//! writes the file again from the code as it is.

use super::*;
use more::Other;

fn probe_query() -> data::Query {
    data::Query { id: "q1".into(), text: "How do solar panels convert sunlight into electricity at night?".into(), vector: Arc::from(vec![0.5f32, -0.25, 0.75, 0.125]) }
}

/// A request of the nine one-node engines as JSON: HTTP (method, URL, body), SQL, or Redis commands (each argument as text, or hex where it is not UTF-8).
fn more_req(r: &more::Req) -> Json {
    match r {
        more::Req::Http { method, url, ctype, body } => {
            let body = serde_json::from_slice::<Json>(body).unwrap_or_else(|_| Json::String(String::from_utf8_lossy(body).into_owned()));
            json!({"method": method.as_str(), "url": url, "ctype": ctype, "body": body})
        }
        more::Req::Sql { url, sql } => json!({"url": url, "sql": sql}),
        more::Req::Redis { url, cmds } => {
            let arg = |a: &Vec<u8>| match std::str::from_utf8(a) {
                Ok(s) => Json::String(s.to_string()),
                Err(_) => Json::String(format!("hex:{}", a.iter().map(|b| format!("{b:02x}")).collect::<String>())),
            };
            json!({"url": url, "cmds": cmds.iter().map(|c| c.iter().map(arg).collect::<Vec<_>>()).collect::<Vec<_>>()})
        }
    }
}

/// Every engine's request of every kind for `p`, as one JSON object.
pub(crate) fn render_all(p: &Probe) -> Json {
    let mut out = serde_json::Map::new();
    for (name, engine, paid) in [("strata", Engine::Strata, false), ("es", Engine::Es, false), ("es-paid", Engine::Es, true), ("os", Engine::Os, false), ("solr", Engine::Solr, false), ("vespa", Engine::Vespa, false)] {
        let p = Probe { paid, ..p.clone() };
        let mut kinds = serde_json::Map::new();
        for &(kind, sql) in KINDS {
            let r = request(engine, kind, sql, &p).map(|(path, body)| json!({"path": path, "body": body}));
            let native = (kind == "key lookup").then(|| native_get(engine, p.key)).flatten().map(|(path, body)| json!({"path": path, "body": body}));
            kinds.insert(kind.to_string(), json!({"request": r, "native": native, "unsupported": unsupported(engine, kind)}));
        }
        out.insert(name.to_string(), Json::Object(kinds));
    }
    for o in [Other::Meili, Other::Typesense, Other::Manticore, Other::Qdrant, Other::Weaviate, Other::Milvus, Other::Paradedb, Other::Postgres, Other::Redis] {
        let mut kinds = serde_json::Map::new();
        for &(kind, _) in KINDS {
            let r = more::request(o, kind, p, "http://engine:1").map(|r| more_req(&r));
            let native = (kind == "key lookup").then(|| more::native_get(o, p, "http://engine:1")).flatten().map(|r| more_req(&r));
            kinds.insert(kind.to_string(), json!({"request": r, "native": native, "unsupported": more::unsupported(o, kind)}));
        }
        out.insert(o.name().to_string(), Json::Object(kinds));
    }
    Json::Object(out)
}

#[test]
fn requests_match_the_snapshot() {
    let q = probe_query();
    let seen = vec!["d7".to_string(), "d9".to_string(), "d11".to_string()];
    let item = [0.25f32, 0.25, -0.5, 0.75];
    let p = Probe::new(&q, "d42", 41, &item, &seen, 2001, false);
    let got = serde_json::to_string_pretty(&render_all(&p)).unwrap() + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/engine-compare/testdata/requests.json");
    if std::env::var("ENGINE_COMPARE_SNAPSHOT").as_deref() == Ok("update") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).expect("testdata/requests.json (ENGINE_COMPARE_SNAPSHOT=update writes it)");
    if got != want {
        let line = got.lines().zip(want.lines()).position(|(a, b)| a != b).unwrap_or(0);
        panic!(
            "the requests changed from testdata/requests.json, first at line {}:\n  now:  {}\n  was:  {}\n(ENGINE_COMPARE_SNAPSHOT=update writes the file again if the change is meant)",
            line + 1,
            got.lines().nth(line).unwrap_or(""),
            want.lines().nth(line).unwrap_or("")
        );
    }
}

/// Filters and prefixes other than `run`'s reach every engine's request:
/// each filtered kind names the new values and none of the old ones.
#[test]
fn knobs_reach_every_engine() {
    let q = probe_query();
    let seen = vec!["d7".to_string(), "d9".to_string(), "d11".to_string()];
    let item = [0.25f32, 0.25, -0.5, 0.75];
    let base = Probe::new(&q, "d42", 41, &item, &seen, 2001, false);
    let knobs = Knobs { year_min: 2017, cats: (1 << 5) | (1 << 41), rating_min: 4.25, similar_rating: 2.75, prefix_len: 2 };
    let was = render_all(&base);
    let now = render_all(&Probe { knobs, ..base.clone() });
    for (engine, kinds) in now.as_object().unwrap() {
        // Manticore's SQL is form-encoded: its spaces and commas decoded, so that `LIMIT%2010` is not taken for 2010.
        let text = |kind: &str| kinds[kind]["request"].to_string().replace("%20", " ").replace("%2C", ",");
        let old = |kind: &str| was[engine][kind]["request"].to_string().replace("%20", " ").replace("%2C", ",");
        if kinds["bm25 + filter"]["unsupported"].is_null() && !kinds["bm25 + filter"]["request"].is_null() {
            let t = text("bm25 + filter");
            assert!(t.contains("2017") && t.contains("c05") && t.contains("c41"), "{engine}: bm25 + filter: {t}");
            assert!(!t.contains("2010") && !t.contains("c00") && !t.contains("c03"), "{engine}: bm25 + filter still has run's filter: {t}");
        }
        if !kinds["vector + filter"]["request"].is_null() {
            let t = text("vector + filter");
            assert!(t.contains("4.25"), "{engine}: vector + filter: {t}");
        }
        if !kinds["similar items"]["request"].is_null() {
            let t = text("similar items");
            assert!(t.contains("2.75"), "{engine}: similar items: {t}");
        }
        // The probe's last word is "night": 2 letters, "ni", where run sends "nig".
        if !kinds["prefix"]["request"].is_null() {
            let (t, o) = (text("prefix"), old("prefix"));
            assert!(t != o && t.contains("ni") && !t.contains("nig"), "{engine}: prefix: {t}");
        }
        for kind in ["bm25", "vector", "hybrid (RRF)", "key lookup", "sql aggregate"] {
            assert_eq!(text(kind), old(kind), "{engine}: {kind} has no filter of the knobs, so it stays as it was");
        }
    }
}
