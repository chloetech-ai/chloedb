//! `chloedb-server grow`: a database served by one server becomes a cluster
//! on this machine at the same address, its rows spread over the shards,
//! and the same command starts the cluster again.

use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// A base port whose cluster's ports (it, and 10, 11, 20, 21, 30 and 31
/// above it) are free now, none of them another test's here.
fn base() -> u16 {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let start = 41_000 + (std::process::id() % 300) as u16 * 40;
    (0..40u16)
        .map(|k| start.wrapping_add(40 * (n + 4 * k)) % 60_000 + 1_024)
        .find(|&p| [0, 10, 11, 20, 21, 30, 31].iter().all(|d| std::net::TcpListener::bind(("127.0.0.1", p + d)).is_ok()))
        .expect("a free block of ports")
}

fn grow(dir: &std::path::Path, listen: &str, shards: Option<usize>) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_chloedb-server"));
    cmd.current_dir(dir).args(["grow", "--db", "films.chloedb", "--listen", listen]);
    if let Some(n) = shards {
        cmd.args(["--shards", &n.to_string()]);
    }
    cmd.env("CHLOEDB_FETCH_MODELS", "0").env_remove("CHLOEDB_TOKEN").stdout(Stdio::null()).stderr(Stdio::piped()).spawn().expect("grow starts")
}

async fn health(http: &reqwest::Client, url: &str) -> Option<Value> {
    http.get(format!("{url}/v1/health")).send().await.ok()?.json().await.ok()
}

/// Waits until the coordinator answers with every row and each shard
/// holds some of them.
async fn spread(http: &reqwest::Client, url: &str, shards: usize, rows: u64) -> Value {
    let t = Instant::now();
    loop {
        if let Some(h) = health(http, url).await {
            let each: Vec<u64> = h["shards"].as_array().map(|a| a.iter().map(|s| s["docs"].as_u64().unwrap_or(0)).collect()).unwrap_or_default();
            if h["role"] == "coordinator" && h["tables"]["films"] == rows && each.len() == shards && each.iter().all(|&n| n > 0) {
                return h;
            }
            assert!(t.elapsed() < Duration::from_secs(90), "the rows did not spread over {shards} shards: {h}");
        }
        assert!(t.elapsed() < Duration::from_secs(90), "{url} did not answer");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// Sends `grow` a signal and waits for it to exit, which it does on its
/// own, having stopped its servers: 0 once it has started them, 130 (Ctrl+C)
/// or 143 (SIGTERM) while it starts them.
fn stop(mut c: Child, signal: i32) -> ExitStatus {
    unsafe { libc::kill(c.id() as i32, signal) };
    let t = Instant::now();
    loop {
        if let Some(status) = c.try_wait().expect("wait") {
            assert!(status.signal().is_none() && matches!(status.code(), Some(0 | 130 | 143)), "grow did not stop by itself on signal {signal}: {status}");
            return status;
        }
        assert!(t.elapsed() < Duration::from_secs(30), "grow did not stop on signal {signal}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Asserts that no server of the cluster of `shards` shards at `p`
/// answers within 5 s, over HTTP or gRPC.
async fn gone(http: &reqwest::Client, p: u16, shards: u16) {
    let t = Instant::now();
    let answering = || async {
        let mut on = Vec::new();
        for port in std::iter::once(p).chain((1..=shards).map(|i| p + 10 * i)) {
            if health(http, &format!("http://127.0.0.1:{port}")).await.is_some() {
                on.push(port);
            }
        }
        for port in (1..=shards).map(|i| p + 10 * i + 1) {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                on.push(port);
            }
        }
        on
    };
    loop {
        let on = answering().await;
        if on.is_empty() {
            return;
        }
        assert!(t.elapsed() < Duration::from_secs(5), "servers still answer on ports {on:?} after grow stopped");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `films.chloedb` in `dir`, with `rows` rows.
fn films(dir: &std::path::Path, rows: u64) {
    let csv: String = std::iter::once("id,title,year\n".to_string()).chain((1..=rows).map(|i| format!("{i},film number {i},{}\n", 1950 + i))).collect();
    std::fs::write(dir.join("films.csv"), csv).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_chloedb-server"))
        .current_dir(dir)
        .args(["import", "--db", "films.chloedb", "--no-meaning", "films.csv"])
        .env("CHLOEDB_FETCH_MODELS", "0")
        .output()
        .unwrap();
    assert!(out.status.success(), "import: {}", String::from_utf8_lossy(&out.stderr));
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_grows_into_a_cluster_at_its_address_and_starts_again() {
    let tmp = tempfile::tempdir().unwrap();
    let rows: u64 = 60;
    films(tmp.path(), rows);
    let p = base();
    let listen = format!("127.0.0.1:{p}");
    let url = format!("http://{listen}");
    let http = client();

    let g = grow(tmp.path(), &listen, Some(2));
    spread(&http, &url, 2, rows).await;
    let q: Value = http.post(format!("{url}/v1/sql")).json(&json!({"sql": "SELECT count(*) FROM films WHERE year > 1980"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(q["rows"][0][0], json!(rows - 30), "{q}");
    let listed: Value = serde_json::from_slice(&std::fs::read(tmp.path().join("films.chloedb.cluster/cluster.json")).unwrap()).unwrap();
    assert_eq!(listed["shards"].as_array().map(Vec::len), Some(2), "{listed}");
    stop(g, libc::SIGINT);
    gone(&http, p, 2).await;

    // Started again with no --shards, it is the cluster cluster.json lists.
    let g = grow(tmp.path(), &listen, None);
    let h = spread(&http, &url, 2, rows).await;
    assert_eq!(h["shards"].as_array().map(Vec::len), Some(2), "{h}");
    stop(g, libc::SIGINT);
    gone(&http, p, 2).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ctrl_c_while_it_starts_stops_every_server_it_started() {
    let tmp = tempfile::tempdir().unwrap();
    let rows: u64 = 60;
    films(tmp.path(), rows);
    let p = base();
    let listen = format!("127.0.0.1:{p}");
    let url = format!("http://{listen}");
    let http = client();
    let g = grow(tmp.path(), &listen, Some(2));
    spread(&http, &url, 2, rows).await;
    stop(g, libc::SIGINT);
    gone(&http, p, 2).await;

    // Started again to grow by a shard, and stopped as soon as the
    // coordinator answers, before the new shard has joined.
    let g = grow(tmp.path(), &listen, Some(3));
    let t = Instant::now();
    while health(&http, &url).await.is_none() {
        assert!(t.elapsed() < Duration::from_secs(60), "{url} did not answer");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop(g, libc::SIGINT);
    gone(&http, p, 3).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sigterm_stops_every_server_it_started() {
    let tmp = tempfile::tempdir().unwrap();
    let rows: u64 = 60;
    films(tmp.path(), rows);
    let p = base();
    let listen = format!("127.0.0.1:{p}");
    let url = format!("http://{listen}");
    let http = client();
    let g = grow(tmp.path(), &listen, Some(2));
    spread(&http, &url, 2, rows).await;
    let status = stop(g, libc::SIGTERM);
    assert_eq!(status.code(), Some(0), "{status}");
    gone(&http, p, 2).await;
}
