//! The simulator against a real chloedb-server: each served impression is a
//! logged, exploring query, and the list, the propensities and the clicks
//! it gets are, bit for bit, those the fast mode draws for the same
//! request, under every logging policy; the log read back through the
//! export, labelled by the clicks written as events, is what was served.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! `../chloedb-server/target/{ci,release}/chloedb-server`), and is skipped
//! without it unless `CHLOEDB_EXPERIMENTS_NEED_SERVER` is set (as CI does).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

use chloedb_experiments::click::Model;
use chloedb_experiments::policy::Policy;
use chloedb_experiments::server::Server;
use chloedb_experiments::sim::{Impression, World};
use chloedb_experiments::site::{Query, Site};
use chloedb_experiments::users::{Population, Shape};

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

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const WORDS: [&str; 16] = ["tent", "lamp", "boot", "cord", "mug", "kettle", "chair", "rope", "map", "stove", "pack", "knife", "torch", "flask", "sock", "hat"];

#[tokio::test(flavor = "multi_thread")]
async fn served_is_fast_and_the_log_is_what_was_served() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_EXPERIMENTS_NEED_SERVER").is_none(), "no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        eprintln!("skipped: no chloedb-server binary (build chloedb-server, or set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let child = Command::new(&bin)
        .args(["--db", &dir.path().join("x.strata").to_string_lossy(), "--listen", &format!("127.0.0.1:{port}"), "--grpc", "off"])
        .env("CHLOEDB_START_APPS", "off")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("chloedb-server");
    let _guard = Running(child);
    let server = Server::new(&format!("http://127.0.0.1:{port}"), None);
    let t0 = Instant::now();
    while server.health().await.is_err() {
        assert!(t0.elapsed() < Duration::from_secs(60), "chloedb-server did not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // A site: 400 rows of three words each, 24 queries of two, graded by
    // how many of a query's words a row's title holds.
    server.sql("CREATE TABLE docs (id KEYWORD PRIMARY KEY, title TEXT, body TEXT)", json!({})).await.unwrap();
    let rows: Vec<(String, String)> = (0..400).map(|i| (format!("d{i}"), format!("{} {} {}", WORDS[i % 16], WORDS[(i * 7 + 3) % 16], WORDS[(i * 5 + 1) % 13]))).collect();
    let ops = rows.iter().map(|(id, t)| json!({"op": "upsert", "table": "docs", "doc": {"id": id, "title": t, "body": t}})).collect();
    server.write(ops).await.unwrap();
    let queries: Vec<Query> = (0..24).map(|q| Query { id: format!("q{q}"), text: format!("{} {}", WORDS[q % 16], WORDS[(q * 3 + 5) % 16]), weight: 1.0 / (q + 1) as f64 }).collect();
    let grades: Vec<HashMap<String, u8>> = queries
        .iter()
        .map(|q| {
            let ws: Vec<&str> = q.text.split(' ').collect();
            rows.iter().filter_map(|(id, t)| { let n = ws.iter().filter(|w| t.split(' ').any(|x| x == **w)).count() as u8; (n > 0).then_some((id.clone(), n)) }).collect()
        })
        .collect();
    let site = Site { name: "toy".into(), table: "docs".into(), queries, grades, max_grade: 2 };
    let rank = "bm25(title, :t, syntax => 'plain') + 0.01 * bm25(body, :t, syntax => 'plain')".to_string();

    let policies = [
        json!({"policy": "softmax", "tau": 1.5, "top": 12, "by": "score"}),
        json!({"policy": "softmax", "tau": 0.7, "top": 8, "by": "rank"}),
        json!({"policy": "epsilon", "epsilon": 0.4, "top": 10}),
        json!({"policy": "shuffle", "top": 6}),
        json!({"policy": "swap", "top": 9}),
        json!({"policy": "none"}),
    ];
    for (n, pj) in policies.iter().enumerate() {
        let policy = Policy::from_json(pj).unwrap();
        let depth = World::depth(&policy, 5);
        let ranked = World::rank_all(&server, &site, &rank, depth).await.unwrap();
        assert!(ranked.iter().all(|r| r.keys.len() == depth), "every query ranks {depth} rows");
        let w = World::new(site.clone(), Population::new(300, 5, Shape::default()), Model::Dbn, policy, 5, rank.clone(), ranked);
        let run = format!("t{n}");
        let (log, events) = (format!("exp_log_{run}"), format!("exp_events_{run}"));
        server.sql(&format!("CREATE TABLE {events} (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD, action KEYWORD, ts INT, dwell_ms FLOAT)"), json!({})).await.unwrap();
        let mut served: Vec<Impression> = Vec::new();
        let mut ops = Vec::new();
        for i in 0..150 {
            let (imp, o) = w.served(&server, &run, &log, &events, i).await.unwrap_or_else(|e| panic!("{pj}: {e}"));
            // Bit for bit what the fast mode draws for this request.
            let fast = w.fast(&run, i);
            assert_eq!(Impression { took_ms: imp.took_ms, ..fast }, imp, "{pj} impression {i}");
            assert!(imp.propensity.iter().all(|p| *p > 0.0 && *p <= 1.0));
            served.push(imp);
            ops.extend(o);
        }
        assert!(!ops.is_empty(), "{pj}: some clicks");
        server.write(ops).await.unwrap();
        let back = w.read_back(&server, &run, &log, &events).await.unwrap();
        assert_eq!(back.len(), served.len(), "{pj}");
        for (b, s) in back.iter().zip(&served) {
            assert_eq!((b.i, &b.shown, &b.propensity, &b.prefix, &b.click, &b.satisfied), (s.i, &s.shown, &s.propensity, &s.prefix, &s.click, &s.satisfied), "{pj} impression {}", s.i);
        }
    }
}
