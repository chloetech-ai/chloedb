//! `chloectx sync` against a real chloedb-server: two machines of one
//! project, each numbering its own records, and a third that pulls both.
//!
//! It needs the chloedb-server binary (`CHLOEDB_SERVER_BIN`, else
//! chloedb-server's `target/ci` or `target/release`); without it, it says
//! so and passes.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chloectx::embed::HashEmbedder;
use chloectx::record::{Kind, Record};
use chloectx::store::{Store, StoreOptions};
use chloectx::sync::{self, SyncState};

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN") {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn store(dir: &Path, name: &str) -> Store {
    Store::open(&dir.join(name), Arc::new(HashEmbedder { dim: 32 }), StoreOptions { sync: false, ..Default::default() }).unwrap()
}

/// Records numbered `seqs`, with ids that say whose they are.
fn records(who: &str, seqs: impl Iterator<Item = i64>) -> Vec<Record> {
    seqs.map(|seq| Record {
        id: format!("{who}-{seq:05}"),
        session: format!("{who}-session"),
        kind: Kind::Note,
        tag: String::new(),
        turn: 1,
        seq,
        ts: 1_700_000_000_000 + seq,
        importance: 0.5,
        files: String::new(),
        text: format!("note {seq} from {who}"),
        ctx: String::new(),
        links: Vec::new(),
    })
    .collect()
}

fn load(s: &Store, recs: &[Record]) {
    let texts: Vec<String> = recs.iter().map(|r| r.embed_text()).collect();
    let refs: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
    let embs = s.embedder.embed(&refs).unwrap();
    s.bulk_insert(recs, embs).unwrap();
}

#[test]
fn machines_that_number_alike_sync_every_record() {
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let _server = Proc(
        Command::new(&bin)
            .args(["--db", &dir.path().join("s.chloedb").to_string_lossy(), "--listen", &format!("127.0.0.1:{port}"), "--grpc", "off", "--no-fetch-models"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let url = format!("http://127.0.0.1:{port}");
    let t0 = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(30), "the server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let home = dir.path().join("home");
    sync::setup(&home, &url, None, "", &[]).unwrap();
    let remote = sync::remote(&home).unwrap();
    let project = "acme/shop";

    // A holds 1..=500, B one record numbered 500 too: the server holds
    // 501 rows, two of them 500, and a page of 500 ends between them.
    let a = store(dir.path(), "a.strata");
    load(&a, &records("a", 1..=500));
    let mut sa = SyncState::default();
    assert_eq!(sync::push(&remote, &a, project, &mut sa).unwrap(), 500);
    let b = store(dir.path(), "b.strata");
    load(&b, &records("b", 500..=500));
    let mut sb = SyncState::default();
    // Its batch is numbered as one of A's was: a key of the numbers alone
    // made the server refuse it as a key used for another request.
    assert_eq!(sync::push(&remote, &b, project, &mut sb).unwrap(), 1, "B's push");
    assert_eq!(remote.count(project).unwrap(), 501);

    // C pulls both machines' records, the second 500 too.
    let c = store(dir.path(), "c.strata");
    let mut sc = SyncState::default();
    assert_eq!(sync::pull(&remote, &c, project, &mut sc).unwrap(), 501);
    assert_eq!(c.len(), 501);
    let got = c.get(&["a-00500".to_string(), "b-00500".to_string()]).unwrap();
    assert_eq!(got.len(), 2, "both records numbered 500");

    // What C records next is pushed, and what it pulled goes back as the
    // same rows, not as new ones.
    load(&c, &records("c", 501..=503));
    let sent = sync::push(&remote, &c, project, &mut sc).unwrap();
    assert!(sent >= 3, "C's own records were sent: {sent}");
    assert_eq!(remote.count(project).unwrap(), 504);
    // And a second pull stores nothing new.
    assert_eq!(sync::pull(&remote, &c, project, &mut sc).unwrap(), 0);
}
