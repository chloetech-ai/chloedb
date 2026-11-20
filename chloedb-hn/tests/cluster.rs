//! chloedb-hn against a real chloedb-server: `serve --local` starts one,
//! loads a small archive of stories, comments and people into it (with
//! autocomplete's rows from `complete`), and the test asks what the page
//! asks: searches, questions in a sentence answered without a model,
//! completions as a question is typed, and exact text.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release}/chloedb-server`) and the
//! embedding model `potion-base-8M` (the server fetches it, or
//! `chloedb-server fetch-model potion-base-8M`), and is skipped without the
//! binary unless `CHLOEDB_HN_NEED_SERVER` is set (as CI does).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use chloedb_hn::prepare::{comment_row, story_row};
use serde_json::{json, Value};

const TOKEN: &str = "hn-test-token";

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN") {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Microseconds at noon of a day.
fn at(y: i32, m: u32, d: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp_micros()
}

const EXP_TOKEN: &str = "experiments-token";

/// (id, author, date, title, url, points, comments)
const STORIES: &[(u32, &str, (i32, u32, u32), &str, &str, i64, i64)] = &[
    (101, "steveklabnik", (2015, 5, 15), "Announcing Rust 1.0", "https://blog.rust-lang.org/2015/05/15/Rust-1.0.html", 1400, 610),
    (102, "alice", (2019, 3, 2), "Why I switched from Go to Rust", "https://alice.dev/go-to-rust", 320, 250),
    (103, "bob", (2021, 4, 14), "Rust in the Linux kernel", "https://lwn.net/Articles/853717/", 850, 930),
    (104, "alice", (2020, 7, 9), "Ask HN: Is Rust worth learning in 2020?", "", 150, 410),
    (105, "satoshi", (2009, 2, 3), "Bitcoin: A Peer-to-Peer Electronic Cash System", "https://bitcoin.org/bitcoin.pdf", 40, 12),
    (106, "dan", (2013, 11, 27), "Bitcoin passes $1,000", "https://www.coindesk.com/bitcoin-1000", 510, 480),
    (107, "erin", (2017, 12, 17), "Bitcoin futures start trading", "https://www.wsj.com/bitcoin-futures", 260, 300),
    (108, "frank", (2023, 6, 1), "Show HN: A faster Node.js parser in C++", "https://github.com/frank/fastparse", 95, 31),
    (109, "grace", (2023, 9, 14), "PostgreSQL 16 released", "https://www.postgresql.org/about/news/postgresql-16-released-2715/", 700, 290),
    (110, "heidi", (2007, 1, 9), "Apple announces the iPhone", "https://www.apple.com/iphone", 600, 2100),
    (111, "ivan", (2022, 11, 30), "ChatGPT: Optimizing language models for dialogue", "https://openai.com/blog/chatgpt", 2900, 1500),
    (112, "judy", (2018, 6, 4), "Microsoft to acquire GitHub for $7.5B", "https://news.microsoft.com/github", 2000, 1300),
    (113, "grace", (2016, 1, 7), "Postgres full text search is good enough", "https://rachbelaid.com/postgres-full-text-search", 410, 120),
    (114, "mallory", (2024, 2, 20), "Show HN: Kubernetes operator for Postgres backups", "https://github.com/mallory/pgop", 80, 22),
];

/// (id, author, story, parent, text)
const COMMENTS: &[(u32, &str, u32, u32, &str)] = &[
    (201, "carol", 103, 103, "I have run Rust in production for three years; the borrow checker caught bugs our C code shipped with."),
    (202, "dave", 103, 201, "The kernel needs a stable ABI before Rust drivers can be more than experiments."),
    (203, "carol", 102, 102, "Rust's async story was the hardest part when we moved from Go, but the compiler errors are excellent."),
    (204, "carol", 104, 104, "Yes. Learning Rust changed how I write C++ too: ownership is the idea worth taking home."),
    (205, "eve", 106, 106, "I bought bitcoin at $12 and sold at $30. Do not ask me for advice."),
    (206, "dave", 109, 109, "Logical replication in PostgreSQL 16 from standbys is the feature I waited for."),
    (207, "eve", 110, 110, "No keyboard, no stylus, AT&T only: this will never sell."),
];

fn write_rows(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let mut f = std::fs::File::create(dir.join("hn_stories.jsonl")).unwrap();
    let title_of = |id: u32| STORIES.iter().find(|s| s.0 == id).unwrap().3;
    for &(id, by, (y, m, d), title, url, points, comments) in STORIES {
        let html = if url.is_empty() { "I keep reading about Rust. Is it worth learning for backend work, or is Go enough?" } else { "" };
        writeln!(f, "{}", story_row(id, 1, by, at(y, m, d), title, html, url, points, comments)).unwrap();
    }
    let mut f = std::fs::File::create(dir.join("hn_comments.jsonl")).unwrap();
    for (i, &(id, by, story, parent, text)) in COMMENTS.iter().enumerate() {
        let s = STORIES.iter().find(|s| s.0 == story).unwrap();
        let parent_by = if parent == story { s.1 } else { COMMENTS.iter().find(|c| c.0 == parent).unwrap().1 };
        let depth = if parent == story { 1 } else { 2 };
        let replies = COMMENTS.iter().filter(|c| c.3 == id).count();
        let (y, m, d) = s.2;
        writeln!(f, "{}", comment_row(id, by, at(y, m, d) + 3_600_000_000 * (i as i64 + 1), text, parent, parent_by, story, title_of(story), depth, 0, replies, replies as u32)).unwrap();
    }
    let mut f = std::fs::File::create(dir.join("hn_users.jsonl")).unwrap();
    let mut people: Vec<&str> = STORIES.iter().map(|s| s.1).chain(COMMENTS.iter().map(|c| c.1)).collect();
    people.sort();
    people.dedup();
    for who in people {
        let st = STORIES.iter().filter(|s| s.1 == who).count() as i64;
        let cm = COMMENTS.iter().filter(|c| c.1 == who).count() as i64;
        let pts: i64 = STORIES.iter().filter(|s| s.1 == who).map(|s| s.5).sum();
        // Enough to be completed as a person: five stories or fifty comments.
        let (st, cm) = if who == "carol" { (st, 60) } else { (st, cm) };
        writeln!(f, "{}", json!({"id": who, "stories": st, "comments": cm, "points": pts, "first": "2007-01-01T00:00:00Z", "last": "2024-01-01T00:00:00Z", "replies_got": 0, "talked_to": 0})).unwrap();
    }
}

struct App {
    child: Child,
    url: String,
}

impl App {
    fn start(dir: &Path, rows: &Path, bin: &Path) -> App {
        let (port, strata) = (free_port(), free_port());
        let child = Command::new(env!("CARGO_BIN_EXE_chloedb-hn"))
            .args(["serve", "--local"])
            .arg(dir)
            .arg("--server-bin")
            .arg(bin)
            .arg("--rows")
            .arg(rows)
            .args(["--listen", &format!("127.0.0.1:{port}"), "--local-listen", &format!("127.0.0.1:{strata}"), "--token", TOKEN, "--experiments-token", EXP_TOKEN, "--experiment-index", "v2=hn_stories", "--explore-share", "1", "--explore-top", "3"])
            .args(["--model", "potion-base-8M", "--comment-model", "none"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-hn");
        App { child, url: format!("http://127.0.0.1:{port}") }
    }

    fn stop(mut self) {
        // SAFETY: a signal to a child this test started.
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(30) {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        panic!("chloedb-hn did not stop on SIGTERM");
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // After a failed assertion too: asked to stop, so that its server stops with it.
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGTERM);
            }
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(20) && self.child.try_wait().ok().flatten().is_none() {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = self.child.kill();
        }
    }
}

async fn get(http: &reqwest::Client, app: &App, path: &str, q: &[(&str, &str)]) -> Value {
    let r = http.get(format!("{}{path}", app.url)).bearer_auth(TOKEN).query(q).send().await.unwrap_or_else(|e| panic!("{path}: {e}"));
    let status = r.status();
    let body = r.text().await.unwrap();
    assert!(status.is_success(), "{path} {q:?}: {status} {body}");
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{path}: {e}: {body}"))
}

async fn search(http: &reqwest::Client, app: &App, q: &str) -> Value {
    get(http, app, "/v1/search", &[("q", q)]).await
}

fn titles(v: &Value) -> Vec<String> {
    v["stories"].as_array().unwrap().iter().map(|s| s["title"].as_str().unwrap_or("").to_string()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn searches_questions_and_completions() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_HN_NEED_SERVER").is_none(), "CHLOEDB_HN_NEED_SERVER is set and there is no chloedb-server binary");
        eprintln!("skipped: no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let rows = tmp.path().join("rows");
    write_rows(&rows);
    let out = Command::new(env!("CARGO_BIN_EXE_chloedb-hn")).args(["complete", "--rows"]).arg(&rows).output().unwrap();
    assert!(out.status.success(), "complete: {}", String::from_utf8_lossy(&out.stderr));
    assert!(rows.join("hn_complete.jsonl").exists());

    let app = App::start(&tmp.path().join("db"), &rows, &bin);
    let http = reqwest::Client::builder().timeout(Duration::from_secs(60)).build().unwrap();
    // The tables are made and loaded before the API answers.
    let t0 = Instant::now();
    loop {
        if let Ok(r) = http.get(format!("{}/healthz", app.url)).send().await {
            if r.status().is_success() {
                break;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(240), "chloedb-hn did not answer in four minutes");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // Words: every story about Rust, the most discussed and voted first among them.
    let r = search(&http, &app, "rust").await;
    assert_eq!(r["total_stories"], 4, "{r}");
    let t = titles(&r);
    assert!(t.iter().take(4).any(|x| x == "Announcing Rust 1.0"), "{t:?}");
    assert!(r["answer"].is_null(), "a search is not a question: {}", r["answer"]);
    // The comments that speak of it, with their story's title.
    assert!(r["comments"].as_array().unwrap().iter().any(|c| c["author"] == "carol"), "{}", r["comments"]);

    // Instant, a keystroke at a time: the last word as the start of one,
    // by points while it is typed, by the words once it is finished.
    let r = get(&http, &app, "/v1/search", &[("q", "postg"), ("mode", "instant")]).await;
    assert_eq!(titles(&r), ["PostgreSQL 16 released", "Postgres full text search is good enough", "Show HN: Kubernetes operator for Postgres backups"], "{r}");
    let r = get(&http, &app, "/v1/search", &[("q", "rust lin"), ("mode", "instant")]).await;
    assert_eq!(titles(&r), ["Rust in the Linux kernel"], "{r}");
    let r = get(&http, &app, "/v1/search", &[("q", "bitcoin "), ("mode", "instant")]).await;
    assert_eq!(r["mode"], "instant");
    assert_eq!(titles(&r).len(), 3, "{r}");
    assert_eq!(r["queries"].as_array().unwrap().len(), 1, "one statement a keystroke: {}", r["queries"]);

    // Questions in a sentence, answered by SQL with no model.
    let r = search(&http, &app, "which post has received the most comments").await;
    assert_eq!(r["answer"]["kind"], "top", "{}", r["answer"]);
    assert_eq!(r["answer"]["item"]["title"], "Apple announces the iPhone", "{}", r["answer"]);

    let r = search(&http, &app, "what is the most upvoted story about bitcoin").await;
    assert_eq!(r["answer"]["item"]["title"], "Bitcoin passes $1,000", "{}", r["answer"]);

    let r = search(&http, &app, "what was the first story about bitcoin").await;
    assert_eq!(r["answer"]["item"]["title"], "Bitcoin: A Peer-to-Peer Electronic Cash System", "{}", r["answer"]);

    let r = search(&http, &app, "how many stories about rust").await;
    assert_eq!(r["answer"]["kind"], "count", "{}", r["answer"]);
    assert_eq!(r["answer"]["n"], 4, "{}", r["answer"]);

    let r = search(&http, &app, "who posted the most stories about rust").await;
    assert_eq!(r["answer"]["kind"], "people", "{}", r["answer"]);
    assert_eq!(r["answer"]["people"][0]["author"], "alice", "{}", r["answer"]);
    assert_eq!(r["answer"]["people"][0]["n"], 2, "{}", r["answer"]);

    let r = search(&http, &app, "who commented the most about rust").await;
    assert_eq!(r["answer"]["people"][0]["author"], "carol", "{}", r["answer"]);

    let r = search(&http, &app, "which sites are posted the most about bitcoin").await;
    assert_eq!(r["answer"]["kind"], "sites", "{}", r["answer"]);
    assert_eq!(r["answer"]["sites"].as_array().unwrap().len(), 3, "{}", r["answer"]);

    let r = search(&http, &app, "top 2 stories about postgres in 2023").await;
    assert_eq!(r["answer"]["kind"], "top", "{}", r["answer"]);
    assert_eq!(r["answer"]["item"]["title"], "PostgreSQL 16 released", "{}", r["answer"]);

    // Exact text, punctuation and all, by the substring index.
    let r = search(&http, &app, "`C++`").await;
    let t = titles(&r);
    assert!(t.iter().any(|x| x.contains("C++")), "{t:?}");
    assert!(t.iter().all(|x| x.contains("C++")), "only titles that hold it: {t:?}");

    // Completions: questions as they are typed, phrases of titles, people.
    let s = get(&http, &app, "/v1/suggest", &[("q", "which post has")]).await;
    let qs: Vec<&str> = s["questions"].as_array().unwrap().iter().filter_map(|x| x.as_str()).collect();
    assert!(qs.iter().any(|q| q.starts_with("which post has")), "{s}");
    // A question's topic is completed where the question wants it.
    let s = get(&http, &app, "/v1/suggest", &[("q", "which post about ru")]).await;
    assert!(s["questions"].as_array().unwrap().iter().any(|q| q == "which post about rust has the most comments"), "{s}");
    let s = get(&http, &app, "/v1/suggest", &[("q", "how many st")]).await;
    assert!(s["questions"].as_array().unwrap().iter().any(|q| q == "how many stories mention "), "an open question for the topic to come: {s}");
    let s = get(&http, &app, "/v1/suggest", &[("q", "rus")]).await;
    assert!(s["phrases"].as_array().unwrap().iter().any(|p| p["text"] == "rust"), "{s}");
    let s = get(&http, &app, "/v1/suggest", &[("q", "postgres")]).await;
    let qs: Vec<&str> = s["questions"].as_array().unwrap().iter().filter_map(|x| x.as_str()).collect();
    assert!(qs.iter().any(|q| q.contains("postgres")), "questions about the topic typed: {s}");
    let s = get(&http, &app, "/v1/suggest", &[("q", "car")]).await;
    assert!(s["people"].as_array().unwrap().iter().any(|p| p["text"] == "carol"), "{s}");

    // The manifest and the view take the token too.
    let r = http.get(format!("{}/app.json", app.url)).send().await.unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(get(&http, &app, "/app.json", &[]).await["maturity"], "demo");

    // A thread, from its story down.
    let th = get(&http, &app, "/v1/item/103", &[]).await;
    assert_eq!(th["story"]["title"], "Rust in the Linux kernel", "{th}");

    // Experiments: the Experiments app's own token, and nobody else's.
    let exp = |path: &str| http.get(format!("{}{path}", app.url));
    assert_eq!(exp("/v1/experiment").bearer_auth(TOKEN).send().await.unwrap().status(), 401);
    let d: Value = exp("/v1/experiment").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert!(d["allowed"] == true && d["site"] == "hn", "{d}");
    let keys: Vec<&str> = d["knobs"].as_array().unwrap().iter().filter_map(|k| k["key"].as_str()).collect();
    for k in ["understand", "meaning", "typos", "votes", "rank", "max_per_site", "index"] {
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
    let ids = |v: &Value| v["rows"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    // As it is, the arm is the site's search.
    let (st, as_is) = es("rust", json!({})).await;
    assert!(st.is_success(), "{as_is}");
    let site = search(&http, &app, "rust").await;
    let site_ids: Vec<String> = site["stories"].as_array().unwrap().iter().take(5).map(|s| s["id"].as_i64().unwrap().to_string()).collect();
    assert_eq!(ids(&as_is), site_ids, "{as_is}");
    assert!(as_is["rows"][0]["snippet"].as_str().unwrap().contains("points"), "{as_is}");
    // Words only finds the stories that say rust; votes that do not count
    // and a ranking of its own order the same stories otherwise.
    let (_, words) = es("rust", json!({"meaning": false})).await;
    assert_eq!(words["total"], 4, "{words}");
    let (_, flat) = es("rust", json!({"votes": 0, "tiers": false, "meaning": false})).await;
    let mut a = ids(&words);
    let mut b = ids(&flat);
    a.sort();
    b.sort();
    assert_eq!(a, b, "{flat}");
    let (_, newest) = es("rust", json!({"rank": "decay(created, now(), 157680000)", "tiers": false, "meaning": false})).await;
    assert_eq!(ids(&newest), ["103", "104", "102", "101"], "newest first: {newest}");
    // Words as typed: "by alice" is two more words, not a filter.
    let (_, by) = es("rust by alice", json!({})).await;
    assert!(ids(&by).iter().all(|i| i == "102" || i == "104"), "{by}");
    let (_, typed) = es("rust by alice", json!({"understand": false})).await;
    assert!(ids(&typed).iter().any(|i| i != "102" && i != "104") || ids(&typed).is_empty(), "{typed}");
    // Another index, and knobs it refuses.
    assert!(es("rust", json!({"index": "v2"})).await.0.is_success());
    for bad in [json!({"index": "v3"}), json!({"votes": 9}), json!({"rank": "1; DROP TABLE hn_stories"}), json!({"nope": true})] {
        let (st, v) = es("rust", bad.clone()).await;
        assert_eq!(st, 400, "{bad}: {v}");
    }
    // Opening a story, and the stories personas are written from.
    let it: Value = exp("/v1/experiment/item/103").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert!(it["text"].as_str().unwrap().contains("borrow checker"), "its comments too: {it}");
    let sm: Value = exp("/v1/experiment/sample?n=10&seed=1").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert!(sm["rows"].as_array().unwrap().len() >= 5, "{sm}");

    // Exploring: the page's search is logged with its top shuffled, a
    // story opened from it is kept, and the log reads them back; an API
    // search (no log=1) is not logged.
    let d: Value = exp("/v1/experiment").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
    assert_eq!(d["log"]["policy"]["policy"], "epsilon", "{d}");
    let plain = get(&http, &app, "/v1/search", &[("q", "rust"), ("only", "story")]).await;
    assert!(plain["log"].is_null(), "{plain}");
    let page = get(&http, &app, "/v1/search", &[("q", "rust"), ("only", "story"), ("log", "1")]).await;
    let req = page["log"]["request"].as_str().expect("a logged request").to_string();
    let first = page["stories"][0]["id"].as_i64().unwrap();
    let c = get(&http, &app, "/v1/click", &[("r", &req), ("id", &first.to_string())]).await;
    assert_eq!(c["kept"], true, "{c}");
    let c = get(&http, &app, "/v1/click", &[("r", "made-up"), ("id", "1")]).await;
    assert_eq!(c["kept"], false, "{c}");
    let t0 = std::time::Instant::now();
    loop {
        let l: Value = exp("/v1/experiment/log?since=0").bearer_auth(EXP_TOKEN).send().await.unwrap().json().await.unwrap();
        let r = l["requests"].as_array().unwrap_or_else(|| panic!("{l}"));
        if r.len() == 1 && r[0]["clicks"] == json!([first]) {
            assert_eq!(r[0]["request"], json!(req));
            assert_eq!(r[0]["explored"], true, "{l}");
            let mut ranked: Vec<i64> = r[0]["ranked"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
            let mut shown: Vec<i64> = r[0]["shown"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
            assert_eq!(shown[0], first, "{l}");
            ranked.sort();
            shown.sort();
            assert_eq!(ranked, shown, "shown is the ranked top reordered: {l}");
            break;
        }
        assert!(t0.elapsed() < std::time::Duration::from_secs(10), "the log and its click: {l}");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    app.stop();
}
