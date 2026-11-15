//! DAT-5: the crawler a chloedb-server starts itself (`--start-apps
//! crawler`, as an install with the apps beside the server does) holds a
//! key the server limits to `crawl_*`, and runs each caller's crawl with it
//! narrowed to the caller's tables: the root's crawl of a site fills its
//! table, and a reader starts none.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release,debug}/chloedb-server`), and is
//! skipped without it unless `CHLOEDB_CRAWL_NEED_SERVER` is set.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use axum::response::Html;
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};

const TOKEN: &str = "started-crawler-token";

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN").or_else(|| std::env::var_os("STRATA_SERVER_BIN")) {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release", "debug"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_crawler_a_server_starts_crawls_for_its_callers() {
    let Some(bin) = server_bin().filter(|b| b.exists()) else {
        assert!(std::env::var_os("CHLOEDB_CRAWL_NEED_SERVER").is_none(), "no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        eprintln!("skipped: no chloedb-server binary (build chloedb-server, or set CHLOEDB_SERVER_BIN)");
        return;
    };
    // A site of two pages.
    let site = Router::new()
        .route("/", get(|| async { Html("<html><head><title>Home</title></head><body><h1>Home</h1><p>Hello from the home page.</p><a href=\"/about\">About</a></body></html>") }))
        .route("/about", get(|| async { Html("<html><head><title>About</title></head><body><h1>About</h1><p>All about this little site.</p></body></html>") }))
        .route("/robots.txt", get(|| async { "User-agent: *\nAllow: /\n" }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let seed = format!("http://127.0.0.1:{}/", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, site).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let (port, grpc) = (free_port(), free_port());
    let here = PathBuf::from(env!("CARGO_BIN_EXE_chloedb-crawl")).parent().unwrap().to_path_buf();
    let path = std::env::join_paths(std::iter::once(here).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
    let _server = Kill(
        Command::new(&bin)
            .arg("--db")
            .arg(dir.path().join("srv.chloedb"))
            .args(["--listen", &format!("127.0.0.1:{port}"), "--grpc", &format!("127.0.0.1:{grpc}"), "--token", TOKEN, "--start-apps", "crawler"])
            .env("PATH", path)
            // The server's setting reaches the crawler it starts: a site
            // on the loopback, as an intranet's would be.
            .env("CHLOEDB_CRAWL_ALLOW_PRIVATE", "true")
            .env_remove("CHLOEDB_APPS")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("chloedb-server"),
    );
    let url = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let t0 = Instant::now();
    loop {
        if let Ok(r) = http.get(format!("{url}/v1/apps/crawler/v1/crawls")).bearer_auth(TOKEN).send().await {
            if r.status().is_success() {
                break;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(90), "the crawler chloedb-server started did not answer through it");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let key = |body: Value| {
        let r = http.post(format!("{url}/v1/admin/keys")).bearer_auth(TOKEN).json(&body).send();
        async move {
            let j: Value = r.await.unwrap().json().await.unwrap();
            j["secret"].as_str().unwrap_or_else(|| panic!("no key: {j}")).to_string()
        }
    };
    let reader = key(json!({"name": "reader", "scope": "read"})).await;
    let spec = json!({"seeds": [seed], "max_pages": 5, "sitemaps": false, "table": "site"});
    let r = http.post(format!("{url}/v1/apps/crawler/v1/crawls")).bearer_auth(&reader).json(&spec).send().await.unwrap();
    assert_eq!(r.status(), 403, "a reader starts no crawl: {}", r.text().await.unwrap());

    let r = http.post(format!("{url}/v1/apps/crawler/v1/crawls")).bearer_auth(TOKEN).json(&spec).send().await.unwrap();
    let status = r.status();
    let started: Value = r.json().await.unwrap();
    assert!(status.is_success(), "{status} {started}");
    let (id, table) = (started["id"].as_str().unwrap().to_string(), started["table"].as_str().unwrap().to_string());
    assert_eq!(table, "crawl_site");
    let t0 = Instant::now();
    let done = loop {
        let j: Value = http.get(format!("{url}/v1/apps/crawler/v1/crawls/{id}")).bearer_auth(TOKEN).send().await.unwrap().json().await.unwrap();
        if matches!(j["phase"].as_str(), Some("done" | "failed" | "stopped")) {
            break j;
        }
        assert!(t0.elapsed() < Duration::from_secs(90), "the crawl did not end: {j}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    };
    assert_eq!(done["phase"], "done", "{done}");
    let r = http.post(format!("{url}/v1/query")).bearer_auth(TOKEN).json(&json!({"sql": format!("SELECT COUNT(*) FROM {table}")})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    assert_eq!(j["rows"][0][0], 2, "both pages: {j}");
}
