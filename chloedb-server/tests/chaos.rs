//! The chaos monkey (`chloedb-chaos`): one scenario on a cluster of one
//! shard of three copies, a primary killed while clients search and
//! write, keeps every acknowledged write once, and the results say what
//! the clients saw and when the command center showed it.

use std::net::TcpListener;
use std::process::Command;

use serde_json::Value as Json;

/// A first port whose block (coordinator, control API, servers) is free.
fn free_block() -> u16 {
    let taken = |p: u16| TcpListener::bind(("127.0.0.1", p)).is_err();
    for _ in 0..200 {
        let base = 20_000 + (rand_u16() % 30_000);
        let ports = [base, base + 99].into_iter().chain((0..12).map(|i| base + 100 + i));
        if !ports.into_iter().any(taken) {
            return base;
        }
    }
    panic!("no free block of ports");
}

fn rand_u16() -> u16 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    (t ^ (std::process::id() << 7)) as u16
}

#[test]
fn a_primary_killed_under_writes_loses_no_acknowledged_write() {
    let tmp = tempfile::tempdir().unwrap();
    // As root, each server runs as a uid of its own and must reach its directory.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let dir = tmp.path().join("run");
    let port = free_block();
    let out = Command::new(env!("CARGO_BIN_EXE_chloedb-chaos"))
        .args(["run", "--scenarios", "primary-crash", "--no-firewall", "--shards", "1", "--spares", "0", "--coordinators", "1", "--rows", "2000", "--readers", "2", "--writers", "2"])
        .arg("--dir")
        .arg(&dir)
        .arg("--bin")
        .arg(env!("CARGO_BIN_EXE_chloedb-server"))
        .args(["--port", &port.to_string()])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "chloedb-chaos failed: {log}");
    let results: Json = serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap()).unwrap();
    let r = &results[0];
    assert_eq!(r["name"], "primary-crash");
    assert_eq!(r["check"]["ok"], true, "{}\n{log}", r["check"]);
    assert!(r["check"]["acknowledged"].as_u64().unwrap() > 100, "writes went on: {}", r["check"]);
    // The command center said so, within seconds of the kill.
    let first = r["command_center"]["first_signal_s"].as_f64().expect("Home showed the fault");
    assert!(first < 10.0, "first shown after {first} s: {}", r["command_center"]);
    assert!(r["recovered_s"].is_u64(), "the clients saw 10 s without a failure again: {log}");
    assert!(std::fs::read_to_string(dir.join("results.md")).unwrap().contains("| primary-crash |"));
}
