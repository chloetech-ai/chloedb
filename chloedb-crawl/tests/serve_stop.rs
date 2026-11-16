//! `chloedb-crawl serve --local DIR`, stopped with SIGTERM as soon as its
//! cluster's coordinator answers, stops that cluster's chloedb-servers
//! rather than leaving them running (their parent gone) on their ports.
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release,debug}/chloedb-server`), and is
//! skipped without it unless `CHLOEDB_CRAWL_NEED_SERVER` is set.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// Ports `base + 10 i` and `base + 10 i + 1` free for one shard.
fn free_base() -> u16 {
    for _ in 0..200 {
        let p = free_port();
        let base = p - p % 10;
        if base > 1024 && [0, 1].iter().all(|d| std::net::TcpListener::bind(("127.0.0.1", base + d)).is_ok()) {
            return base;
        }
    }
    panic!("no free ports");
}

/// The processes whose command line names `dir`.
fn servers_in(dir: &str) -> Vec<String> {
    let out = Command::new("pgrep").args(["-f", dir]).output().expect("pgrep");
    String::from_utf8_lossy(&out.stdout).split_whitespace().map(str::to_string).collect()
}

fn answers(url: &str) -> bool {
    let addr = url.trim_start_matches("http://");
    let Ok(mut s) = std::net::TcpStream::connect(addr) else { return false };
    use std::io::{Read, Write};
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if s.write_all(format!("GET /v1/health HTTP/1.0\r\nHost: {addr}\r\n\r\n").as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200") || buf.starts_with("HTTP/1.0 200")
}

#[test]
fn sigterm_right_after_start_stops_the_local_cluster() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_CRAWL_NEED_SERVER").is_none(), "no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        eprintln!("skipped: no chloedb-server binary (build chloedb-server, or set CHLOEDB_SERVER_BIN)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("crawl-cluster-stop");
    let dir_s = dir.to_string_lossy().into_owned();
    let coord = format!("127.0.0.1:{}", free_port());
    let mut child = Command::new(env!("CARGO_BIN_EXE_chloedb-crawl"))
        .args(["serve", "--listen", &format!("127.0.0.1:{}", free_port()), "--local", &dir_s, "--local-listen", &coord, "--port-base", &free_base().to_string()])
        .arg("--server-bin")
        .arg(&bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // As the guide does: wait for the coordinator, then stop the crawler.
    let deadline = Instant::now() + Duration::from_secs(120);
    while !answers(&coord) {
        assert!(child.try_wait().unwrap().is_none(), "chloedb-crawl serve exited before its cluster answered");
        assert!(Instant::now() < deadline, "the cluster did not answer at {coord}");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!servers_in(&dir_s).is_empty(), "the cluster's servers run");
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "chloedb-crawl serve did not stop");
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let left = servers_in(&dir_s);
        if left.is_empty() {
            break;
        }
        if Instant::now() > deadline {
            for p in &left {
                let _ = Command::new("kill").args(["-9", p]).status();
            }
            panic!("chloedb-crawl serve left its chloedb-servers running: {left:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
