//! chloedb-arxiv against real chloedb-servers: `serve --local --local-graph`
//! starts two (the search server and the graph server, as in production),
//! loads a small made-up arXiv into them, makes the recommender, and every
//! page the view reads is asked for and checked: search by words and
//! meaning, a paper with its citations and what to read next, a person and
//! their coauthors, a topic, the atlas, what readers open becoming the
//! recommender's events and their "For you", and a feed's preview.
//!
//! The papers embed with ChloeDB's `hash:512` model, so no model is
//! downloaded and none is called. It needs the chloedb-server binary
//! (`CHLOEDB_SERVER_BIN`, else `../chloedb-server/target/{ci,release}/
//! chloedb-server`), and is skipped without it unless
//! `CHLOEDB_ARXIV_NEED_SERVER` is set.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TOKEN: &str = "arxiv-test-token";
const EXP_TOKEN: &str = "arxiv-experiments-token";

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN").or_else(|| std::env::var_os("STRATA_SERVER_BIN")) {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Two fields of fifteen papers each: sequential recommendation (cs.IR)
/// and quantum error correction (quant-ph). Each paper cites the two
/// before it in its field, and readers read four papers of one field in
/// order, so the graph and the recommender both point within a field.
const REC: [&str; 15] = [
    "Self-attentive sequential recommendation with item embeddings",
    "Transformers for sequential recommendation of next items",
    "Contrastive learning for sequential recommendation",
    "Session-based recommendation with graph neural networks",
    "Generative retrieval for recommendation with semantic IDs",
    "Large language models as sequential recommenders",
    "Click-through rate prediction with feature interactions",
    "Negative sampling for implicit feedback recommendation",
    "Long user histories in sequential recommendation",
    "Diffusion models for sequential recommendation",
    "Cold-start recommendation from item content",
    "Multi-behavior sequential recommendation in e-commerce",
    "Evaluating sequential recommendation fairly",
    "Scaling laws of sequential recommendation models",
    "Retrieval and ranking stages of industrial recommenders",
];
const QEC: [&str; 15] = [
    "Surface codes for quantum error correction",
    "Decoding the surface code with neural networks",
    "Magic state distillation with fewer qubits",
    "Quantum low-density parity-check codes",
    "Fault-tolerant thresholds of topological codes",
    "Lattice surgery for logical qubit operations",
    "Erasure qubits and quantum error correction",
    "Bosonic codes for continuous-variable qubits",
    "Real-time decoding of quantum error correction",
    "Leakage in superconducting qubit error correction",
    "Color codes and transversal gates",
    "Quantum error mitigation versus correction",
    "Concatenated codes for fault-tolerant quantum computing",
    "Logical qubits beyond break-even",
    "Benchmarking quantum error correction on hardware",
];

fn rec_id(i: usize) -> String {
    format!("2401.{:05}", 10001 + i)
}
fn qec_id(i: usize) -> String {
    format!("2402.{:05}", 20001 + i)
}

fn write_rows(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let mut papers = Vec::new();
    let mut cites = Vec::new();
    let mut events = Vec::new();
    let mut coauthors = Vec::new();
    let fields: [(&[&str; 15], fn(usize) -> String, &str, &str, i64, [&str; 3]); 2] = [
        (&REC, rec_id, "cs.IR", "cs", 0, ["ada recsys", "bo ranker", "cy embed"]),
        (&QEC, qec_id, "quant-ph", "quant-ph", 1, ["dee qubit", "eli surface", "fay decoder"]),
    ];
    for (titles, id, cat, area, topic, people) in fields.iter() {
        for (i, t) in titles.iter().enumerate() {
            let who: Vec<&str> = vec![people[i % 3], people[(i + 1) % 3]];
            let abs = format!("We study {}. Our method improves on earlier work in {} and is evaluated on standard benchmarks.", t.to_lowercase(), if *topic == 0 { "recommender systems" } else { "quantum computing" });
            papers.push(json!({
                "id": id(i), "title": t, "abstract": abs,
                "authors": who.join(", "), "people": who, "first_author": who[0], "nauthors": 2,
                "cats": [cat], "pcat": cat, "area": area, "areas": [area],
                "created": format!("2024-0{}-{:02}T12:00:00Z", 1 + topic, 1 + i), "updated": format!("2024-0{}-{:02}T12:00:00Z", 1 + topic, 1 + i),
                "year": 2024, "month": format!("2024-0{}", 1 + topic), "versions": 1,
                "techniques": [], "domains": [], "kinds": [],
                "cites": 30 - i, "pop": 1.0, "refs": i.min(2), "cited_in": if i < 13 { 2 } else { 15 - i - 1 },
                "license": "arXiv", "topic": topic,
            }));
            for back in 1..=2 {
                if i >= back {
                    cites.push(json!({"id": format!("{}>{}", id(i), id(i - back)), "src": id(i), "dst": id(i - back)}));
                }
            }
        }
        for r in 0..12 {
            for k in 0..4 {
                let item = id((r + k) % 15);
                let usr = format!("doi:10.0/{area}-{r}");
                events.push(json!({"id": format!("{usr}|{item}"), "usr": usr, "item": item, "ts": format!("2024-03-01T00:0{k}:00Z"), "kind": "cite"}));
            }
        }
        for a in 0..3 {
            for b in 0..3 {
                if a != b {
                    coauthors.push(json!({"id": format!("{}>{}", people[a], people[b]), "src": people[a], "dst": people[b], "w": 5.0}));
                }
            }
        }
    }
    let authors: Vec<Value> = fields
        .iter()
        .flat_map(|f| f.5.iter().map(move |p| (p, f.2)))
        .map(|(p, cat)| json!({"id": p, "name": p.split(' ').map(|w| w[..1].to_uppercase() + &w[1..]).collect::<Vec<_>>().join(" "), "papers": 10, "first_author": 5, "cites": 100, "h": 4, "first": "2024-01-01T00:00:00Z", "last": "2024-02-15T00:00:00Z", "recent": 10, "cats": [cat], "techniques": [], "coauthors": 2}))
        .collect();
    let topics = vec![
        json!({"id": 0, "label": "Sequential recommendation", "about": "Recommending the next item from what a user did.", "terms": ["recommendation", "sequential"], "field": 0, "field_label": "Computer Science", "papers": 15, "recent": 15, "growth": 1.2, "cites": 300, "first": 2024, "peak": 2024, "by_year": {"2024": 15}, "cats": ["cs.IR"], "techniques": [], "domains": [], "x": 0.25, "y": 0.5}),
        json!({"id": 1, "label": "Quantum error correction", "about": "Codes and decoders that protect qubits.", "terms": ["quantum", "codes"], "field": 1, "field_label": "Quantum Physics", "papers": 15, "recent": 15, "growth": 1.1, "cites": 300, "first": 2024, "peak": 2024, "by_year": {"2024": 15}, "cats": ["quant-ph"], "techniques": [], "domains": [], "x": 0.75, "y": 0.5}),
    ];
    let cats = vec![
        json!({"id": "cs.IR", "name": "Information Retrieval", "area": "cs", "area_name": "Computer Science", "papers": 15, "cites": 300, "recent": 15, "growth": 1.0, "by_year": {"2024": 15}}),
        json!({"id": "quant-ph", "name": "Quantum Physics", "area": "quant-ph", "area_name": "Quantum Physics", "papers": 15, "cites": 300, "recent": 15, "growth": 1.0, "by_year": {"2024": 15}}),
    ];
    let complete = vec![
        json!({"id": "p:sequential recommendation", "kind": "phrase", "text": "sequential recommendation", "n": 12, "weight": 9.0}),
        json!({"id": "p:surface codes", "kind": "phrase", "text": "surface codes", "n": 3, "weight": 4.0}),
        json!({"id": "a:ada recsys", "kind": "author", "text": "Ada Recsys", "n": 10, "weight": 5.0, "ref": "ada recsys"}),
    ];
    let put = |name: &str, rows: &[Value]| {
        let mut f = std::fs::File::create(dir.join(format!("{name}.jsonl"))).unwrap();
        for r in rows {
            writeln!(f, "{r}").unwrap();
        }
    };
    put("ax_papers", &papers);
    put("ax_authors", &authors);
    put("ax_cites", &cites);
    put("ax_coauthors", &coauthors);
    put("ax_events", &events);
    put("ax_topics", &topics);
    put("ax_cats", &cats);
    put("ax_complete", &complete);
}

struct App {
    child: Child,
    url: String,
}

impl Drop for App {
    fn drop(&mut self) {
        // SAFETY: a signal to a child this test started; it stops its servers.
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(20) {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
    }
}

async fn get(http: &reqwest::Client, app: &App, path: &str) -> Value {
    let r = http.get(format!("{}{path}", app.url)).bearer_auth(TOKEN).send().await.unwrap_or_else(|e| panic!("GET {path}: {e}"));
    let status = r.status();
    let body = r.text().await.unwrap();
    assert!(status.is_success(), "GET {path}: {status} {body}");
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("GET {path}: {e}: {body}"))
}

async fn post(http: &reqwest::Client, app: &App, path: &str, body: Value) -> Value {
    let r = http.post(format!("{}{path}", app.url)).bearer_auth(TOKEN).json(&body).send().await.unwrap_or_else(|e| panic!("POST {path}: {e}"));
    let status = r.status();
    let text = r.text().await.unwrap();
    assert!(status.is_success(), "POST {path}: {status} {text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("POST {path}: {e}: {text}"))
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|p| p.get("id").and_then(|x| x.as_str()).map(str::to_string)).collect()).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn two_servers_every_page() {
    let Some(bin) = server_bin() else {
        if std::env::var_os("CHLOEDB_ARXIV_NEED_SERVER").is_some() {
            panic!("no chloedb-server binary: set CHLOEDB_SERVER_BIN or build ../chloedb-server");
        }
        eprintln!("skipped: no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let rows = tmp.path().join("rows");
    write_rows(&rows);
    let (port, search, graph) = (free_port(), free_port(), free_port());
    let child = Command::new(env!("CARGO_BIN_EXE_chloedb-arxiv"))
        .arg("serve")
        .arg("--local")
        .arg(tmp.path().join("search"))
        .arg("--local-graph")
        .arg(tmp.path().join("graph"))
        .arg("--rows")
        .arg(&rows)
        .arg("--server-bin")
        .arg(&bin)
        .args(["--model", "hash:512", "--token", TOKEN, "--experiments-token", EXP_TOKEN, "--experiment-index", "v2=ax_papers"])
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .args(["--local-listen", &format!("127.0.0.1:{search}")])
        .args(["--local-graph-listen", &format!("127.0.0.1:{graph}")])
        .env_remove("CHLOEDB_URL")
        .env_remove("CHLOEDB_ARXIV_GRAPH")
        .env("CHLOEDB_MODELS", tmp.path().join("models"))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("chloedb-arxiv");
    let app = App { child, url: format!("http://127.0.0.1:{port}") };
    let http = reqwest::Client::new();
    let t0 = Instant::now();
    loop {
        if let Ok(r) = http.get(format!("{}/healthz", app.url)).send().await {
            if r.status().is_success() {
                break;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(180), "the app did not answer in 3 minutes");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // Both servers hold their tables.
    let st = get(&http, &app, "/v1/status").await;
    assert!(st.to_string().contains("ax_papers"), "{st}");

    // Search by words and meaning: each field's query finds its own papers first.
    let rec: Vec<String> = (0..15).map(rec_id).collect();
    let qec: Vec<String> = (0..15).map(qec_id).collect();
    let s = get(&http, &app, "/v1/search?q=sequential%20recommendation&facets=1").await;
    let found = ids(&s["papers"]);
    assert!(!found.is_empty(), "{s}");
    assert!(found.iter().take(5).all(|id| rec.contains(id)), "{found:?}");
    let s = get(&http, &app, "/v1/search?q=surface%20codes%20decoding").await;
    let found = ids(&s["papers"]);
    assert!(qec.contains(&found[0]), "{found:?}");
    // The words of the query are marked in what is shown.
    assert!(s.to_string().contains("<mark>"), "{s}");

    // A paper: what it cites and who cites it from the graph server,
    // read next fused from the recommender's counts, the citation walk and
    // the nearest by meaning, all within its field.
    let p = get(&http, &app, &format!("/v1/paper/{}", rec_id(7))).await;
    assert_eq!(p["paper"]["id"], rec_id(7));
    let refs = ids(&p["references"]);
    assert!(refs.contains(&rec_id(6)) && refs.contains(&rec_id(5)), "{refs:?}");
    let cited = ids(&p["cited_by"]);
    assert!(cited.contains(&rec_id(8)) && cited.contains(&rec_id(9)), "{cited:?}");
    let next = ids(&p["read_next"]);
    assert!(!next.is_empty(), "{p}");
    assert!(next.iter().take(5).all(|id| rec.contains(id)), "{next:?}");
    assert!(!next.contains(&rec_id(7)), "{next:?}");
    assert!(!ids(&p["cited_with"]).is_empty(), "the recommender's pairs: {p}");
    assert_eq!(p["topic"]["id"], 0);

    // A person and the people they write with.
    let a = get(&http, &app, "/v1/author/ada%20recsys").await;
    assert!(!a["coauthors"].as_array().map(|x| x.is_empty()).unwrap_or(true), "{a}");

    // A topic and the atlas.
    let t = get(&http, &app, "/v1/topic/1").await;
    assert!(t.to_string().contains("Quantum error correction"), "{t}");
    let at = get(&http, &app, "/v1/atlas").await;
    assert_eq!(at["topics"].as_array().map(|x| x.len()), Some(2), "{at}");

    // Suggestions as one types.
    let sg = get(&http, &app, "/v1/suggest?q=sequ").await;
    assert!(sg.to_string().contains("sequential recommendation"), "{sg}");

    // What a reader opens is an event of the recommender; For you then
    // recommends from that field and not what they read.
    let user = "reader-test-1";
    for paper in [qec_id(2), qec_id(3)] {
        post(&http, &app, "/v1/event", json!({"user": user, "paper": paper, "kind": "open"})).await;
    }
    let fy = get(&http, &app, &format!("/v1/foryou?user={user}")).await;
    assert_eq!(fy["events"], 2, "{fy}");
    let got = ids(&fy["papers"]);
    assert!(!got.is_empty(), "{fy}");
    assert!(got.iter().take(5).all(|id| qec.contains(id)), "{got:?}");
    assert!(!got.contains(&qec_id(2)) && !got.contains(&qec_id(3)), "{got:?}");
    let lib = get(&http, &app, &format!("/v1/library?user={user}")).await;
    assert!(lib.is_object(), "{lib}");

    // A feed's preview: the papers an issue would hold, chosen with no model call.
    let pv = post(&http, &app, "/v1/feeds/preview", json!({"config": {"interests": ["sequential recommendation"], "days": 400, "papers": 5}})).await;
    let chosen = ids(&pv["papers"]);
    assert!(!chosen.is_empty(), "{pv}");
    assert!(chosen.iter().all(|id| rec.contains(id)), "{chosen:?}");

    // What to cite: a draft citing two papers of a field gets more of
    // that field (cited with them, or close in meaning), not what it cites.
    let draft = format!("We study sequential recommendation with transformers.\n\nReferences\n[1] arXiv:{}\n[2] https://arxiv.org/abs/{}\n", rec_id(3), rec_id(4));
    let cite = post(&http, &app, "/v1/cite", json!({"text": draft})).await;
    assert_eq!(cite["found"]["ids"], 2, "{cite}");
    assert_eq!(ids(&cite["refs"]).len(), 2, "{cite}");
    let more = ids(&cite["papers"]);
    assert!(!more.is_empty(), "{cite}");
    assert!(more.iter().take(3).all(|id| rec.contains(id)), "{more:?}");
    assert!(!more.contains(&rec_id(3)) && !more.contains(&rec_id(4)), "{more:?}");

    // A standing question is kept, listed, read as RSS and dropped by its
    // reader (the made-up papers are from 2024: nothing new to read).
    let w = post(&http, &app, "/v1/watches", json!({"user": "reader-one", "question": "Tell me when someone uses transformers for sequential recommendation"})).await;
    let wid = w["id"].as_str().unwrap().to_string();
    assert_eq!(w["read"], 0, "{w}");
    let mine = get(&http, &app, "/v1/watches?user=reader-one").await;
    assert_eq!(ids(&mine["watches"]), vec![wid.clone()], "{mine}");
    let one = get(&http, &app, &format!("/v1/watches/{wid}?user=reader-one")).await;
    assert_eq!(one["mine"], true, "{one}");
    let rss = http.get(format!("{}/watches/{wid}/rss.xml", app.url)).bearer_auth(TOKEN).send().await.unwrap().text().await.unwrap();
    assert!(rss.contains("sequential recommendation") && rss.contains("<rss"), "{rss}");
    post(&http, &app, &format!("/v1/watches/{wid}/delete"), json!({"user": "reader-one"})).await;
    assert!(ids(&get(&http, &app, "/v1/watches?user=reader-one").await["watches"]).is_empty());

    // People to work with, reviewers for a paper and for a pasted abstract
    // (its authors left out), and the lab pages (no labs loaded: empty).
    let co = get(&http, &app, "/v1/collab/ada%20recsys").await;
    assert_eq!(co["person"]["id"], "ada recsys", "{co}");
    let rv = get(&http, &app, &format!("/v1/reviewers/{}", rec_id(7))).await;
    assert!(rv["people"].as_array().unwrap().iter().all(|p| p["id"] != "ada recsys"), "{rv}");
    let rv = post(&http, &app, "/v1/reviewers", json!({"title": "Transformers for sequential recommendation", "abstract": "We model what a user does next with a transformer over the items they saw.", "authors": "Ada Recsys"})).await;
    assert_eq!(rv["authors"][0], "ada recsys", "{rv}");
    assert!(rv["people"].as_array().unwrap().iter().all(|p| p["id"] != "ada recsys"), "{rv}");
    assert!(get(&http, &app, "/v1/labs/of/2401.00001").await["labs"].as_array().unwrap().is_empty());

    // For and against: a claim of one word is refused, and a kept answer
    // is served as it was kept (no model runs here).
    let r = http.get(format!("{}/v1/claim?q=scaling", app.url)).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let kept = json!({"claim": "transformers help sequential recommendation", "since": 0, "for": [{"id": rec_id(3), "title": "t", "quote": "q", "why": "w"}], "against": [], "mixed": [], "read": 1, "none": 0, "others": [], "dropped": 0, "kept": false});
    let doc = json!({"id": "claim:transformers help sequential recommendation", "paper": "", "style": "claim", "text": kept.to_string(), "model": "m", "at": "2026-11-16T00:00:00Z", "ms": 1.0});
    let w = http.post(format!("http://127.0.0.1:{graph}/v1/write")).bearer_auth(TOKEN).json(&json!({"ops": [{"op": "upsert", "table": "ax_notes", "doc": doc}]})).send().await.unwrap();
    assert!(w.status().is_success(), "{}", w.text().await.unwrap());
    let c = get(&http, &app, "/v1/claim?q=Transformers%20help%20sequential%20recommendation!").await;
    assert_eq!(c["kept"], true, "{c}");
    assert_eq!(c["for"][0]["id"], rec_id(3), "{c}");

    // Explain at my level: a kept explanation is served for its level with
    // no model, and an unknown level is read as a practitioner's.
    let note = json!({"explain": "Kept for a student.", "terms": [{"term": "attention", "meaning": "m"}]});
    let doc = json!({"id": format!("explain:practitioner:{}", rec_id(2)), "paper": rec_id(2), "style": "explain-practitioner", "text": note.to_string(), "model": "m", "at": "2026-11-16T00:00:00Z", "ms": 1.0});
    let w = http.post(format!("http://127.0.0.1:{graph}/v1/write")).bearer_auth(TOKEN).json(&json!({"ops": [{"op": "upsert", "table": "ax_notes", "doc": doc}]})).send().await.unwrap();
    assert!(w.status().is_success(), "{}", w.text().await.unwrap());
    let e = get(&http, &app, &format!("/v1/explain/{}?level=wizard", rec_id(2))).await;
    assert_eq!((e["kept"].clone(), e["level"].clone(), e["note"]["explain"].clone()), (json!(true), json!("practitioner"), json!("Kept for a student.")), "{e}");

    // Weekly feeds: a reader's three write themselves, a fourth is refused,
    // and one that does not is saved as before.
    for i in 0..3 {
        let f = post(&http, &app, "/v1/feeds", json!({"user": "reader-weekly", "config": {"name": format!("w{i}"), "interests": ["recommendation"], "weekly": true}})).await;
        assert!(f["id"].as_str().is_some(), "{f}");
    }
    let r = http.post(format!("{}/v1/feeds", app.url)).bearer_auth(TOKEN).json(&json!({"user": "reader-weekly", "config": {"name": "w3", "weekly": true}})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let f = post(&http, &app, "/v1/feeds", json!({"user": "reader-weekly", "config": {"name": "by hand"}})).await;
    assert_eq!(get(&http, &app, &format!("/v1/feeds/{}", f["id"].as_str().unwrap())).await["config"]["weekly"], false);

    // Team feeds: another reader changes a feed only with its team link's
    // key, the feed stays its owner's, and a dropped link stops working.
    let f = post(&http, &app, "/v1/feeds", json!({"user": "reader-owner", "config": {"name": "ours"}})).await;
    let fid = f["id"].as_str().unwrap().to_string();
    let save = |team: &str| http.post(format!("{}/v1/feeds", app.url)).bearer_auth(TOKEN).json(&json!({"id": fid, "user": "reader-mate", "team": team, "config": {"name": "ours, changed"}})).send();
    assert_eq!(save("").await.unwrap().status().as_u16(), 403);
    let key = post(&http, &app, &format!("/v1/feeds/{fid}/team"), json!({"user": "reader-owner"})).await["team"].as_str().unwrap().to_string();
    assert_eq!(key.len(), 32);
    assert_eq!(save(&key).await.unwrap().status().as_u16(), 200);
    let o = get(&http, &app, &format!("/v1/feeds/{fid}?user=reader-owner")).await;
    assert_eq!((o["owner"].clone(), o["config"]["name"].clone(), o["team"].clone()), (json!("reader-owner"), json!("ours, changed"), json!(key)), "{o}");
    assert_eq!(get(&http, &app, &format!("/v1/feeds/{fid}?team={key}")).await["member"], true);
    let other = get(&http, &app, &format!("/v1/feeds/{fid}?team=nope")).await;
    assert_eq!((other["member"].clone(), other["team"].clone()), (json!(false), Value::Null));
    post(&http, &app, &format!("/v1/feeds/{fid}/team"), json!({"user": "reader-owner", "off": true})).await;
    assert_eq!(save(&key).await.unwrap().status().as_u16(), 403);

    // Writing an issue tells its progress: the papers it chose, each step
    // with its phase and the paper it is on, and the issue it kept (no
    // model in these tests, so its paragraphs are empty).
    let f = post(&http, &app, "/v1/feeds", json!({"user": "reader-progress", "config": {"name": "progress", "interests": ["sequential recommendation"], "days": 400, "papers": 3, "style": "bullets", "extras": []}})).await;
    let fid = f["id"].as_str().unwrap().to_string();
    let job = post(&http, &app, &format!("/v1/feeds/{fid}/issue"), json!({"user": "reader-progress"})).await["job"].as_str().unwrap().to_string();
    let mut j = Value::Null;
    for _ in 0..240 {
        j = get(&http, &app, &format!("/v1/jobs/{job}")).await;
        if j["state"] != "running" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let chose = ids(&j["papers"]);
    assert!((1..=3).contains(&chose.len()), "{j}");
    assert_eq!((j["state"].clone(), j["phase"].clone(), j["name"].clone(), j["n"].clone()), (json!("done"), json!("done"), json!("progress"), json!(chose.len())), "{j}");
    assert!(chose.iter().all(|id| rec.contains(id)), "{chose:?}");
    assert!(j["papers"].as_array().unwrap().iter().all(|p| !p["title"].as_str().unwrap_or("").is_empty()), "{j}");
    let phases: Vec<&str> = j["log"].as_array().unwrap().iter().filter_map(|l| l["phase"].as_str()).collect();
    let mut want = vec!["choose", "chosen"];
    want.extend(std::iter::repeat("write").take(chose.len()));
    want.push("save");
    assert_eq!(phases, want, "{j}");
    let on: Vec<Value> = j["log"].as_array().unwrap().iter().filter(|l| l["phase"] == "write").map(|l| l["i"].clone()).collect();
    assert_eq!(on, (0..chose.len()).map(|i| json!(i)).collect::<Vec<_>>(), "{j}");
    assert_eq!(get(&http, &app, &format!("/v1/issue/{}", j["issue"].as_str().unwrap())).await["issue"]["vol"], 1);

    // Likely classics: a judged paper is listed with its judgement and the
    // paper's row, under the models' card.
    let row = json!({"id": rec_id(4), "p": 0.2, "lift": 18.5, "win": 182, "early": 7, "record_cites": 900, "area": "cs", "pcat": "cs.IR", "created": chrono::Utc::now().to_rfc3339(), "why": ["cited in its first 182 days", "code"]});
    let w = http.post(format!("http://127.0.0.1:{search}/v1/write")).bearer_auth(TOKEN).json(&json!({"ops": [{"op": "upsert", "table": "ax_classics", "doc": row}]})).send().await.unwrap();
    assert!(w.status().is_success(), "{}", w.text().await.unwrap());
    let card = json!({"id": "classics:model", "paper": "", "style": "model", "text": json!({"horizon": "2026-03-01", "models": {}}).to_string(), "model": "", "at": "2026-11-16T00:00:00Z", "ms": 0.0});
    let w = http.post(format!("http://127.0.0.1:{graph}/v1/write")).bearer_auth(TOKEN).json(&json!({"ops": [{"op": "upsert", "table": "ax_notes", "doc": card}]})).send().await.unwrap();
    assert!(w.status().is_success(), "{}", w.text().await.unwrap());
    let lc = get(&http, &app, "/v1/classics?area=cs&days=91").await;
    assert_eq!((lc["papers"][0]["id"].clone(), lc["papers"][0]["classic"]["lift"].clone(), lc["model"]["horizon"].clone()), (json!(rec_id(4)), json!(18.5), json!("2026-03-01")), "{lc}");
    assert_eq!(get(&http, &app, "/v1/classics?area=math").await["papers"].as_array().unwrap().len(), 0);

    // Related work: a list's BibTeX with no model, keyed by surname, year and
    // title word; a list of one paper is refused.
    let b = get(&http, &app, &format!("/v1/related/bib?ids={},{},nope", rec_id(1), rec_id(2))).await;
    assert_eq!(b["papers"].as_array().unwrap().len(), 2, "{b}");
    let bib = b["bibtex"].as_str().unwrap();
    assert!(bib.matches("archivePrefix = {arXiv}").count() == 2 && bib.contains(&format!("eprint = {{{}}}", rec_id(1))), "{bib}");
    let r = http.post(format!("{}/v1/related", app.url)).bearer_auth(TOKEN).json(&json!({"text": rec_id(1)})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);

    // The pages of trends, rising, gaps, new topics, spread, code, labs and home answer too.
    for path in ["/v1/home", "/v1/trends?q=recommendation,quantum", "/v1/hot", "/v1/hot/people", "/v1/gaps", "/v1/born", "/v1/spread", "/v1/code", "/v1/labs", "/v1/labs/find?q=university", "/v1/feeds/example", "/app.json"] {
        get(&http, &app, path).await;
    }

    // Experiments: the Experiments app's own token, and nobody else's.
    let exp = |path: &str| http.get(format!("{}{path}", app.url));
    assert_eq!(exp("/v1/experiment").bearer_auth(TOKEN).send().await.unwrap().status(), 401);
    let d: Value = exp("/v1/experiment").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert!(d["allowed"] == true && d["site"] == "arxiv", "{d}");
    let keys: Vec<&str> = d["knobs"].as_array().unwrap().iter().filter_map(|k| k["key"].as_str()).collect();
    for k in ["understand", "meaning", "typos", "authors", "cites", "rank", "max_per_category", "index"] {
        assert!(keys.contains(&k), "{k}: {keys:?}");
    }
    let es = |q: &str, knobs: Value| {
        let h = http.post(format!("{}/v1/experiment/search", app.url)).bearer_auth(EXP_TOKEN).json(&json!({"q": q, "knobs": knobs, "per": 5}));
        async move {
            let r = h.send().await.unwrap();
            let status = r.status();
            let v: Value = r.json().await.unwrap();
            (status, v)
        }
    };
    // As it is, the arm is the site's search.
    let (st, as_is) = es("sequential recommendation", json!({})).await;
    assert!(st.is_success(), "{as_is}");
    let site = get(&http, &app, "/v1/search?q=sequential%20recommendation&facets=0").await;
    assert_eq!(ids(&as_is["rows"]), ids(&site["papers"]).into_iter().take(5).collect::<Vec<_>>(), "{as_is}");
    assert!(as_is["rows"][0]["snippet"].as_str().unwrap().contains("citations"), "{as_is}");
    // A ranking of its own: the most cited first.
    let (_, cited) = es("sequential recommendation", json!({"rank": "cites", "meaning": false})).await;
    assert_eq!(ids(&cited["rows"])[0], rec_id(0), "{cited}");
    // Blending: every abstract says benchmarks (as words, not the kind of
    // paper the site reads it as), the oldest first puts the January field
    // first, and at most two of a category mixes the other in.
    let rank = "1 - decay(created, now(), 157680000)";
    let (_, one) = es("benchmarks", json!({"rank": rank, "understand": false})).await;
    let (_, two) = es("benchmarks", json!({"rank": rank, "understand": false, "max_per_category": 2})).await;
    let cats = |v: &Value| v["rows"].as_array().unwrap().iter().map(|r| if qec.contains(&r["id"].as_str().unwrap().to_string()) { 'q' } else { 'r' }).collect::<String>();
    assert_eq!(cats(&one), "rrrrr", "{one}");
    assert_eq!(cats(&two), "rrqqr", "{two}");
    assert_eq!(ids(&two["rows"])[..3], [rec_id(0), rec_id(1), qec_id(0)], "{two}");
    // Understanding: "by ada recsys" is a filter, or three more words.
    let (_, by) = es("recommendation by ada recsys", json!({})).await;
    assert!(by["understood"].as_array().is_some_and(|n| !n.is_empty()), "{by}");
    assert!(by["rows"].as_array().unwrap().iter().all(|r| r["snippet"].as_str().unwrap().to_lowercase().contains("ada recsys")), "{by}");
    let (_, typed) = es("recommendation by ada recsys", json!({"understand": false})).await;
    assert!(typed["understood"].as_array().is_some_and(|n| n.is_empty()), "{typed}");
    // Another index, and knobs it refuses.
    assert!(es("sequential recommendation", json!({"index": "v2"})).await.0.is_success());
    for bad in [json!({"index": "v3"}), json!({"cites": 9}), json!({"rank": "1; DROP TABLE ax_papers"}), json!({"nope": true})] {
        let (st, v) = es("sequential recommendation", bad.clone()).await;
        assert_eq!(st, 400, "{bad}: {v}");
    }
    // Opening a paper, and the papers personas are written from.
    let it: Value = exp(&format!("/v1/experiment/item/{}", rec_id(0))).bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert!(it["text"].as_str().unwrap().contains("We study"), "{it}");
    let sm: Value = exp("/v1/experiment/sample?n=10&seed=1").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert!(sm["rows"].as_array().unwrap().len() >= 5 && sm["rows"][0]["abstract"].is_string(), "{sm}");
    drop(app);
}
