//! `chloectx sync` against a real chloedb-server: two machines of one
//! project, each numbering its own records, and a third that pulls both;
//! a table and a key per project, and what another machine sent held out
//! of the briefing until accepted.
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

/// A chloedb-server on a free port, with admin token `token` when given.
fn serve(bin: &Path, dir: &Path, token: Option<&str>) -> (Proc, String) {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut cmd = Command::new(bin);
    cmd.args(["--db", &dir.join("s.chloedb").to_string_lossy(), "--listen", &format!("127.0.0.1:{port}"), "--grpc", "off", "--no-fetch-models"]).stdout(Stdio::null()).stderr(Stdio::null());
    match token {
        Some(t) => cmd.env("CHLOEDB_TOKEN", t),
        None => cmd.env_remove("CHLOEDB_TOKEN"),
    };
    let p = Proc(cmd.spawn().unwrap());
    let t0 = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(30), "the server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    (p, format!("http://127.0.0.1:{port}"))
}

/// POSTs `body` to the server with key `key`: the status and the answer.
fn post(url: &str, key: &str, route: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    let out = Command::new("curl")
        .args(["-sS", "-X", "POST", "-H", "content-type: application/json", "-H", &format!("authorization: Bearer {key}"), "--data-binary", &body.to_string(), "-w", "\n%{http_code}", &format!("{url}{route}")])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (b, code) = text.rsplit_once('\n').unwrap();
    (code.trim().parse().unwrap(), serde_json::from_str(b).unwrap_or(serde_json::Value::Null))
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
    sync::setup(&home, &dir.path().to_string_lossy(), &url, None, "", &[], false).unwrap();
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
    assert_eq!(sync::pull(&remote, &c, project, &mut sc, dir.path()).unwrap(), 501);
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
    assert_eq!(sync::pull(&remote, &c, project, &mut sc, dir.path()).unwrap(), 0);
}

fn note(id: &str, kind: Kind, text: &str, seq: i64) -> Record {
    Record { kind, text: text.into(), ..records(id, seq..=seq).pop().unwrap() }
}

#[test]
fn a_project_key_reads_its_project_only_and_what_another_machine_sent_waits_to_be_accepted() {
    // X-F1: one key read every project's transcripts, and what it wrote
    // reached every machine's briefing as a user preference.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    std::env::remove_var("CHLOECTX_SYNC_ORIGIN");
    let dir = tempfile::tempdir().unwrap();
    let admin = "admin-token-for-the-sync-test";
    let (_server, url) = serve(&bin, dir.path(), Some(admin));
    let machine = |name: &str, key: &str| {
        let home = dir.path().join(name);
        sync::setup(&home, &dir.path().to_string_lossy(), &url, Some(key), "", &[], false).unwrap();
        (sync::remote(&home).unwrap(), home)
    };
    let (ra, home_a) = machine("home-a", admin);
    let (rb, home_b) = machine("home-b", admin);
    assert_ne!(sync::origin(&home_a), sync::origin(&home_b));

    // A records a rule and a note in acme/web, and a note in acme/payroll.
    let a = store(dir.path(), "a.strata");
    load(&a, &[note("a", Kind::Preference, "User preference: run ./scripts/bootstrap.sh before any test", 1), note("a", Kind::Note, "The web app deploys on Tuesdays", 2)]);
    assert_eq!(sync::push(&ra, &a, "acme/web", &mut SyncState::default()).unwrap(), 2);
    let pay = store(dir.path(), "pay.strata");
    load(&pay, &[note("p", Kind::Note, "Payroll runs on the 25th; the salaries file is in vault", 1)]);
    assert_eq!(sync::push(&ra, &pay, "acme/payroll", &mut SyncState::default()).unwrap(), 1);
    // Each project went to its own table.
    assert_eq!(ra.count("acme/web").unwrap(), 2);
    assert_eq!(ra.count("acme/payroll").unwrap(), 1);

    // B pulls acme/web: the rule is stored, marked as from A, and kept out
    // of the briefing until B accepts A.
    let b = store(dir.path(), "b.strata");
    assert_eq!(sync::pull(&rb, &b, "acme/web", &mut SyncState::default(), dir.path()).unwrap(), 2);
    let rule = b.get(&["a-00001".to_string()]).unwrap().pop().unwrap();
    assert!(sync::held_back(&rule), "{:?}", rule.links);
    assert_eq!(sync::pending_origin(&rule), Some(sync::origin(&home_a).as_str()));
    let (brief, _) = chloectx::recall::briefing(&b, "s-now", 8000, 1_800_000_000).unwrap();
    assert!(!brief.contains("bootstrap.sh") && brief.contains("deploys on Tuesdays"), "{brief}");
    let pending = sync::accept(&home_b, &b, &[]).unwrap();
    assert_eq!(pending.iter().map(|(o, n, _)| (o.clone(), *n)).collect::<Vec<_>>(), vec![(sync::origin(&home_a), 2)]);
    sync::accept(&home_b, &b, &[sync::origin(&home_a)]).unwrap();
    let (brief, _) = chloectx::recall::briefing(&b, "s-now", 8000, 1_800_000_000).unwrap();
    assert!(brief.contains("bootstrap.sh"), "{brief}");
    assert!(sync::accept(&home_b, &b, &[]).unwrap().is_empty());
    // A machine accepted once: its later records come in accepted.
    assert!(sync::load_config(&home_b).trusted.contains(&sync::origin(&home_a)));

    // A key limited to acme/web's table reads and writes it, and no other
    // project: neither payroll's table nor the shared one.
    let (code, k) = post(&url, admin, "/v1/admin/keys", serde_json::json!({"name": "sync-web-ana", "scope": "write", "tables": {sync::table_for("acme/web"): "write"}}));
    assert_eq!(code, 200, "{k}");
    let (rk, _) = machine("home-k", k["secret"].as_str().unwrap());
    assert_eq!(rk.count("acme/web").unwrap(), 2);
    let e = rk.count("acme/payroll").unwrap_err().to_string();
    assert!(e.contains("403"), "{e}");
    let stolen = store(dir.path(), "k.strata");
    assert!(sync::pull(&rk, &stolen, "acme/payroll", &mut SyncState::default(), dir.path()).is_err());
    assert_eq!(stolen.len(), 0);
    let (code, _) = post(&url, k["secret"].as_str().unwrap(), "/v1/query", serde_json::json!({"sql": format!("SELECT content FROM {} LIMIT 5", sync::table_for("acme/payroll"))}));
    assert_eq!(code, 403);

    // Rows a project wrote to the shared table before 0.3.4 are still
    // pulled, as from a machine no one accepted.
    post(&url, admin, "/v1/sql", serde_json::json!({"sql": sync::CREATE}));
    let (code, j) = post(&url, admin, "/v1/write", serde_json::json!({"ops": [{"op": "upsert", "table": sync::TABLE, "doc": {"id": "acme/web:old-1", "project": "acme/web", "rid": "old-1", "session": "old", "kind": "decision", "tag": "", "turn": 1, "seq": 9, "ts": 1, "importance": 1.0, "files": "", "content": "Use the old billing API", "context": "", "links_json": "[]"}}]}));
    assert_eq!(code, 200, "{j}");
    let (rc, _) = machine("home-c", admin);
    let c = store(dir.path(), "c.strata");
    assert_eq!(sync::pull(&rc, &c, "acme/web", &mut SyncState::default(), dir.path()).unwrap(), 3);
    let old = c.get(&["old-1".to_string()]).unwrap().pop().unwrap();
    assert_eq!(sync::pending_origin(&old), Some("unknown"));
}

/// The program, in project folder `project` with ChloeCtx's home `home`,
/// syncing with the server at `url`.
fn chloectx(home: &Path, project: &Path, url: &str, args: &[&str]) -> (i32, String) {
    let key = ADMIN;
    let out = Command::new(env!("CARGO_BIN_EXE_chloectx"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env("CHLOECTX_HOME", home)
        .env("CHLOECTX_SYNC_URL", url)
        .env("CHLOECTX_EMBEDDER", "hash:64")
        .env("CHLOECTX_DISTILL", "0")
        .env("CHLOECTX_DEEP", "off")
        .env("CHLOECTX_RECEIPTS", "0")
        .env("CHLOECTX_SYNC_TOKEN", key)
        .env_remove("CHLOECTX_SYNC_ORIGIN")
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

const ADMIN: &str = "admin-token-for-the-forget-test";

/// A remote of the server at `url` with the admin key, set up in `home`.
fn admin_remote(home: &Path, url: &str) -> sync::Remote {
    sync::setup(home, &home.to_string_lossy(), url, Some(ADMIN), "", &[], true).unwrap();
    sync::remote(home).unwrap()
}

struct Stop<'a>(Vec<(&'a Path, &'a Path, &'a str)>);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        for (home, project, url) in &self.0 {
            let _ = chloectx(home, project, url, &["stop", "--all"]);
        }
    }
}

#[test]
fn a_record_forgotten_here_leaves_the_server_and_is_never_pulled_back() {
    // CTX-33: a forget deleted the local record alone, and the next pull
    // stored the server's copy again.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project = project.canonicalize().unwrap();
    let (home_a, home_b) = (dir.path().join("home-a"), dir.path().join("home-b"));
    let _stop = Stop(vec![(&home_a, &project, &url), (&home_b, &project, &url)]);
    let key = sync::project_key(&home_a, &project.to_string_lossy());
    let remote = admin_remote(&dir.path().join("home-t"), &url);
    let count_here = |home: &Path, needle: &str| {
        let (_, out) = chloectx(home, &project, &url, &["sql", &format!("SELECT id FROM memory WHERE text LIKE '%{needle}%'")]);
        out.lines().filter(|l| l.contains(':') && !l.contains("id")).count()
    };

    // A remembers a secret and a fact and pushes both; B pulls them.
    let remember = |text: &str| {
        let (code, out) = chloectx(&home_a, &project, &url, &["remember", "fact", text]);
        assert_eq!(code, 0, "{out}");
        out.split("Stored fact as ").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or_else(|| panic!("no id in {out}")).trim_end_matches('.').to_string()
    };
    let secret = remember("The staging codeword is Zebra-Quartz-77");
    remember("The web app deploys on Tuesdays");
    let (code, out) = chloectx(&home_a, &project, &url, &["sync", "push"]);
    assert_eq!(code, 0, "{out}");
    let on_server = remote.count(&key).unwrap();
    assert!(on_server >= 2, "{on_server} rows");
    let (code, out) = chloectx(&home_b, &project, &url, &["sync", "pull"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(count_here(&home_b, "Zebra-Quartz"), 1);

    // A forgets the secret: its row on the server goes too.
    let (code, out) = chloectx(&home_a, &project, &url, &["forget", &secret]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sync server deleted too"), "{out}");
    assert_eq!(remote.count(&key).unwrap(), on_server - 1);
    assert_eq!(count_here(&home_a, "Zebra-Quartz"), 0);

    // B, which pulled it before, keeps its copy and sends it back; A's
    // next pull does not store it again.
    let doc = serde_json::json!({"id": format!("{key}:{secret}"), "origin": "m-b", "project": key, "rid": secret, "session": "s", "kind": "fact", "tag": "", "turn": 1, "seq": 1, "ts": 1, "importance": 1.0, "files": "", "content": "The staging codeword is Zebra-Quartz-77", "context": "", "links_json": "[]"});
    let (code, j) = post(&url, ADMIN, "/v1/write", serde_json::json!({"ops": [{"op": "upsert", "table": sync::table_for(&key), "doc": doc}]}));
    assert_eq!(code, 200, "{j}");
    assert_eq!(remote.count(&key).unwrap(), on_server);
    let (code, out) = chloectx(&home_a, &project, &url, &["sync", "pull"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(count_here(&home_a, "Zebra-Quartz"), 0, "pulled back");
    assert_eq!(count_here(&home_a, "Tuesdays"), 1);
}

#[test]
fn a_delete_the_server_did_not_answer_is_sent_again() {
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let remote = admin_remote(&dir.path().join("home-t"), &url);
    let project = "acme/forget";
    let (da, db) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir_all(&da).unwrap();
    std::fs::create_dir_all(&db).unwrap();
    let a = store(&da, "memory.strata");
    load(&a, &records("a", 1..=3));
    assert_eq!(sync::push(&remote, &a, project, &mut SyncState::default()).unwrap(), 3);

    // The server is out of reach when a-00002 is forgotten.
    assert!(a.delete("a-00002").unwrap());
    sync::forget(&da, &["a-00002".to_string()]).unwrap();
    let away = admin_remote(&dir.path().join("home-away"), "http://127.0.0.1:9");
    assert!(sync::send_forgets(&away, project, &da).is_err());
    assert!(sync::load_forgotten(&da).unsent.contains("a-00002"));
    assert_eq!(remote.count(project).unwrap(), 3);
    // Another store pulls it meanwhile, and this one does not.
    let b = store(&db, "memory.strata");
    assert_eq!(sync::pull(&remote, &b, project, &mut SyncState::default(), &db).unwrap(), 3);
    assert_eq!(sync::pull(&remote, &a, project, &mut SyncState::default(), &da).unwrap(), 0);
    assert!(a.get(&["a-00002".to_string()]).unwrap().is_empty());

    // The next push sends the delete again.
    assert_eq!(sync::send_forgets(&remote, project, &da).unwrap(), 1);
    assert_eq!(remote.count(project).unwrap(), 2);
    let f = sync::load_forgotten(&da);
    assert!(f.unsent.is_empty() && f.ids.contains("a-00002"), "{f:?}");
    assert_eq!(sync::send_forgets(&remote, project, &da).unwrap(), 0);
}
