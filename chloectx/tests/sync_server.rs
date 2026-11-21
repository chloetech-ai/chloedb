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
    assert_eq!(sync::pull(&remote, &c, project, &mut sc, dir.path()).unwrap().stored, 501);
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
    assert_eq!(sync::pull(&remote, &c, project, &mut sc, dir.path()).unwrap().stored, 0);
}

/// WEI-301: records a store kept before private asides were cut out (the
/// prompt, a rule picked from it, the turn's request in a note's context)
/// go to the server without the aside, or not at all.
#[test]
fn a_private_aside_is_not_pushed() {
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), None);
    let home = dir.path().join("home");
    sync::setup(&home, &dir.path().to_string_lossy(), &url, None, "", &[], false).unwrap();
    let remote = sync::remote(&home).unwrap();
    let project = "acme/ledger";
    let prompt = "Off the record, please don't save this anywhere: I'm interviewing at another company next week. Anyway, add a .env.example with placeholders for the staging settings.";
    let ctx = format!("Turn 1 request: {prompt}");
    let rec = |kind: Kind, tag: &str, text: &str, seq: i64| Record { tag: tag.into(), ctx: ctx.clone(), ..note("w", kind, text, seq) };
    let a = store(dir.path(), "a.strata");
    load(&a, &[
        rec(Kind::Prompt, "-", prompt, 1),
        rec(Kind::Preference, "cue", &format!("User preference: {}", &prompt[..96]), 2),
        rec(Kind::Decision, "distilled", "Staging settings are documented in .env.example with placeholders.", 3),
    ]);
    let mut sa = SyncState::default();
    assert_eq!(sync::push(&remote, &a, project, &mut sa).unwrap(), 2, "the rule is not sent");
    let c = store(dir.path(), "c.strata");
    assert_eq!(sync::pull(&remote, &c, project, &mut SyncState::default(), dir.path()).unwrap().stored, 2);
    let got = c.get(&["w-00001".to_string(), "w-00002".to_string(), "w-00003".to_string()]).unwrap();
    assert_eq!(got.len(), 2, "{got:?}");
    for r in &got {
        assert!(!r.text.contains("interviewing") && !r.ctx.contains("interviewing"), "{r:?}");
    }
    assert!(got.iter().any(|r| r.text.contains(".env.example") && r.text.contains("[private aside left out]")), "{got:?}");
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
        // The machine's key, as a remote of no project of its own.
        sync::setup(&home, &dir.path().to_string_lossy(), &url, Some(key), "", &[], true).unwrap();
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
    assert_eq!(sync::pull(&rb, &b, "acme/web", &mut SyncState::default(), dir.path()).unwrap().stored, 2);
    let rule = b.get(&["a-00001".to_string()]).unwrap().pop().unwrap();
    assert!(sync::held_back(&rule), "{:?}", rule.links);
    assert_eq!(sync::pending_origin(&rule), Some(sync::origin(&home_a).as_str()));
    let (brief, _) = chloectx::recall::briefing(&b, "s-now", 8000, 1_800_000_000).unwrap();
    assert!(!brief.contains("bootstrap.sh") && brief.contains("deploys on Tuesdays"), "{brief}");
    let (pending, _) = sync::accept(&home_b, &b, "acme/web", &[], false).unwrap();
    assert_eq!(pending.iter().map(|p| (p.origin.clone(), p.records)).collect::<Vec<_>>(), vec![(sync::origin(&home_a), 1)], "only the rule waits: the note is shown already, labelled (ANA-402)");
    sync::accept(&home_b, &b, "acme/web", &[sync::origin(&home_a)], false).unwrap();
    let (brief, _) = chloectx::recall::briefing(&b, "s-now", 8000, 1_800_000_000).unwrap();
    assert!(brief.contains("bootstrap.sh"), "{brief}");
    assert!(sync::accept(&home_b, &b, "acme/web", &[], false).unwrap().0.is_empty());
    // A machine accepted once: its later records of the project come in accepted.
    assert!(sync::load_config(&home_b).trusts("acme/web", &sync::origin(&home_a)));

    // A key limited to acme/web's table reads and writes it, and no other
    // project: neither payroll's table nor the shared one.
    let (code, k) = post(&url, admin, "/v1/admin/keys", serde_json::json!({"name": "sync-web-ana", "scope": "write", "tables": {sync::table_for("acme/web"): "write"}}));
    assert_eq!(code, 200, "{k}");
    let (rk, _) = machine("home-k", k["secret"].as_str().unwrap());
    assert_eq!(rk.count("acme/web").unwrap(), 2);
    // WEI-403: it is not asked for the shared table it may not read, and
    // an admin's key is.
    assert!(!rk.may_read(sync::TABLE) && rk.may_read(&sync::table_for("acme/web")) && ra.may_read(sync::TABLE));
    // ... so its routine pull makes no refused request for Access and
    // audit to show.
    let refused = || {
        let out = Command::new("curl").args(["-sS", "-H", &format!("authorization: Bearer {admin}"), &format!("{url}/v1/admin/audit?kind=refusal&who=sync-web-ana")]).output().unwrap();
        let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        j["entries"].as_array().unwrap_or_else(|| panic!("{j}")).len()
    };
    let web = store(dir.path(), "k-web.strata");
    sync::pull(&rk, &web, "acme/web", &mut SyncState::default(), dir.path()).unwrap();
    assert_eq!(web.len(), 2);
    assert_eq!(refused(), 0, "a project key's pull asked for what it may not read");
    let e = rk.count("acme/payroll").unwrap_err().to_string();
    assert!(e.contains("403"), "{e}");
    assert!(refused() >= 1, "the audit log lists a refusal");
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
    assert_eq!(sync::pull(&rc, &c, "acme/web", &mut SyncState::default(), dir.path()).unwrap().stored, 3);
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

    // A forgets the secret: its row on the server is replaced by a
    // tombstone, which the server's count leaves out.
    let (code, out) = chloectx(&home_a, &project, &url, &["forget", &secret]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("replaced by a tombstone"), "{out}");
    assert_eq!(remote.count(&key).unwrap(), on_server - 1);
    assert_eq!(count_here(&home_a, "Zebra-Quartz"), 0);

    // WEI-304: B, which pulled it before, forgets it on its next pull, and
    // a machine that pulls for the first time never stores it.
    let (code, out) = chloectx(&home_b, &project, &url, &["sync", "pull"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(count_here(&home_b, "Zebra-Quartz"), 0, "B kept what A forgot");
    assert_eq!(count_here(&home_b, "Tuesdays"), 1);
    let (code, out) = chloectx(&home_b, &project, &url, &["sync", "push"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(remote.count(&key).unwrap(), on_server - 1, "B sent it back");
    let home_c = dir.path().join("home-c");
    let _stop_c = Stop(vec![(&home_c, &project, &url)]);
    let (code, out) = chloectx(&home_c, &project, &url, &["sync", "pull"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!((count_here(&home_c, "Zebra-Quartz"), count_here(&home_c, "Tuesdays")), (0, 1));

    // A machine from before 0.3.9, which pulled it and sends it back,
    // replaces the tombstone; A's next pull does not store it again.
    let doc = serde_json::json!({"id": format!("{key}:{secret}"), "origin": "m-b", "project": key, "rid": secret, "session": "s", "kind": "fact", "tag": "", "turn": 1, "seq": 1, "ts": 1, "importance": 1.0, "files": "", "content": "The staging codeword is Zebra-Quartz-77", "context": "", "links_json": "[]"});
    let (code, j) = post(&url, ADMIN, "/v1/write", serde_json::json!({"ops": [{"op": "upsert", "table": sync::table_for(&key), "doc": doc}]}));
    assert_eq!(code, 200, "{j}");
    assert_eq!(remote.count(&key).unwrap(), on_server);
    let (code, out) = chloectx(&home_a, &project, &url, &["sync", "pull"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(count_here(&home_a, "Zebra-Quartz"), 0, "pulled back");
    assert_eq!(count_here(&home_a, "Tuesdays"), 1);
    // Nor do B's and C's, which forgot it.
    for home in [&home_b, &home_c] {
        let (code, out) = chloectx(home, &project, &url, &["sync", "pull"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(count_here(home, "Zebra-Quartz"), 0, "pulled back");
    }
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
    assert_eq!(sync::pull(&remote, &b, project, &mut SyncState::default(), &db).unwrap().stored, 3);
    assert_eq!(sync::pull(&remote, &a, project, &mut SyncState::default(), &da).unwrap().stored, 0);
    assert!(a.get(&["a-00002".to_string()]).unwrap().is_empty());

    // The next push sends the delete again.
    assert_eq!(sync::send_forgets(&remote, project, &da).unwrap(), 1);
    assert_eq!(remote.count(project).unwrap(), 2);
    let f = sync::load_forgotten(&da);
    assert!(f.unsent.is_empty() && f.ids.contains("a-00002"), "{f:?}");
    assert_eq!(sync::send_forgets(&remote, project, &da).unwrap(), 0);
}

#[test]
fn accept_names_each_machine_by_its_key_and_all_reaches_only_the_machines_listed_now() {
    // WEI-308: `sync accept` listed `m-44fb96669a15: 73 records`, and
    // `accept --all` wrote `trusted: ["*"]`: every machine to come, for
    // every project of the laptop.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    std::env::remove_var("CHLOECTX_SYNC_ORIGIN");
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let (ledger, payroll) = ("acme/ledger", "acme/payroll");
    let machine = |name: &str, key: &str| {
        let home = dir.path().join(name);
        sync::setup(&home, &dir.path().to_string_lossy(), &url, Some(key), "", &[], true).unwrap();
        (sync::remote(&home).unwrap(), home)
    };
    // The key "Make a sync key" makes for alice's laptop.
    let table = sync::table_for(ledger);
    let name = format!("chloectx-sync-alice-laptop-{}", &table[table.len() - 8..]);
    let admin = admin_remote(&dir.path().join("home-admin"), &url);
    admin.create_table(ledger).unwrap();
    let (code, k) = post(&url, ADMIN, "/v1/admin/keys", serde_json::json!({"name": name, "scope": "write", "tables": {table.clone(): "write"}}));
    assert_eq!(code, 200, "{k}");
    let (ra, home_a) = machine("home-alice", k["secret"].as_str().unwrap());
    assert_eq!(ra.key_name().as_deref(), Some(name.as_str()));
    assert_eq!(sync::machine_name(&name), "alice-laptop");
    // carol syncs with the server's own token, which names no one.
    let (rc, home_c) = machine("home-carol", ADMIN);
    assert_eq!(rc.key_name(), None);
    let (rb, home_b) = machine("home-bob", ADMIN);

    let a = store(dir.path(), "a.strata");
    load(&a, &[note("a", Kind::Decision, "Use Postgres SKIP LOCKED for the job queue, not Redis", 1)]);
    sync::push(&ra, &a, ledger, &mut SyncState::default()).unwrap();
    let c = store(dir.path(), "c.strata");
    load(&c, &[note("c", Kind::Preference, "User preference: money is integer cents", 1)]);
    sync::push(&rc, &c, ledger, &mut SyncState::default()).unwrap();

    // bob lists them: alice's by her key's name, carol's by its id.
    let b = store(dir.path(), "b.strata");
    assert_eq!(sync::pull(&rb, &b, ledger, &mut SyncState::default(), dir.path()).unwrap().stored, 2);
    let (waiting, _) = sync::accept(&home_b, &b, ledger, &[], false).unwrap();
    let (oa, oc) = (sync::origin(&home_a), sync::origin(&home_c));
    let labels: Vec<String> = waiting.iter().map(|p| p.label()).collect();
    assert!(labels.contains(&format!("alice-laptop ({oa})")) && labels.contains(&oc), "{labels:?}");
    assert_eq!(waiting.iter().find(|p| p.origin == oa).unwrap().key.as_deref(), Some(name.as_str()));
    // The server's row names the key too, for an admin's SQL.
    let (code, j) = post(&url, ADMIN, "/v1/query", serde_json::json!({"sql": format!("SELECT links_json FROM {table} WHERE origin = :o LIMIT 5"), "params": {"o": oa}}));
    assert_eq!(code, 200, "{j}");
    assert!(j["rows"][0][0].as_str().unwrap().contains(&name), "{j}");

    // By the machine's name; a name that matches no machine says so.
    let (_, unknown) = sync::accept(&home_b, &b, ledger, &["alice-laptop".into(), "dave-laptop".into()], false).unwrap();
    assert_eq!(unknown, vec!["dave-laptop".to_string()]);
    let held = |id: &str| sync::held_back(&b.get(&[id.to_string()]).unwrap().pop().unwrap());
    assert!(!held("a-00001") && held("c-00001"));
    // WEI-306: her decision reads as hers.
    let (brief, _) = chloectx::recall::briefing(&b, "s-now", 8000, 1_800_000_000).unwrap();
    let line = brief.lines().find(|l| l.contains("SKIP LOCKED")).unwrap_or_else(|| panic!("{brief}"));
    assert!(line.contains("from alice-laptop)"), "{line}");

    // --all: the machines listed now, in this project, written by name.
    let (left, _) = sync::accept(&home_b, &b, ledger, &[], true).unwrap();
    assert_eq!(left.iter().map(|p| p.origin.clone()).collect::<Vec<_>>(), vec![oc.clone()]);
    assert!(!held("c-00001"));
    let cfg = sync::load_config(&home_b);
    assert!(cfg.trusted.is_empty() && cfg.accepted.get(ledger).is_some_and(|o| o.contains(&oa) && o.contains(&oc)), "{cfg:?}");

    // A machine that pushes later waits, as does an accepted machine's
    // record of another project.
    let (rd, _) = machine("home-dan", ADMIN);
    let d = store(dir.path(), "d.strata");
    load(&d, &[note("d", Kind::Preference, "User preference: add Redis for the job queue", 1)]);
    sync::push(&rd, &d, ledger, &mut SyncState::default()).unwrap();
    let ap = store(dir.path(), "ap.strata");
    load(&ap, &[note("ap", Kind::Preference, "User preference: payroll runs on the 25th", 1)]);
    sync::push(&rc, &ap, payroll, &mut SyncState::default()).unwrap();
    assert_eq!(sync::pull(&rb, &b, ledger, &mut SyncState::default(), dir.path()).unwrap().stored, 1);
    assert!(held("d-00001"), "a machine that pushed after --all");
    let bp = store(dir.path(), "bp.strata");
    assert_eq!(sync::pull(&rb, &bp, payroll, &mut SyncState::default(), dir.path()).unwrap().stored, 1);
    assert!(sync::held_back(&bp.get(&["ap-00001".to_string()]).unwrap().pop().unwrap()), "carol was accepted in acme/ledger only");

    // `*`, as --all wrote it before, accepts no machine any more.
    let cfg = sync::SyncConfig { trusted: vec!["*".into()], ..Default::default() };
    assert!(cfg.trusted_everyone() && !cfg.trusts(ledger, "m-anyone"));
    let cfg = sync::SyncConfig { trusted: vec![oa.clone()], ..Default::default() };
    assert!(cfg.trusts(payroll, &oa), "a machine accepted before 0.3.9 stays accepted");
}

/// A machine of the server at `url` with key `key`, set up in `dir/name`.
fn machine_of(dir: &Path, url: &str, name: &str, key: &str) -> (sync::Remote, PathBuf) {
    let home = dir.join(name);
    sync::setup(&home, &dir.to_string_lossy(), url, Some(key), "", &[], true).unwrap();
    (sync::remote(&home).unwrap(), home)
}

/// A key that writes `project`'s table alone, named as the command
/// center's "Make a sync key" names one for machine `who`.
fn sync_key(url: &str, project: &str, who: &str) -> (String, String) {
    let table = sync::table_for(project);
    let name = format!("chloectx-sync-{who}-{}", &table[table.len() - 8..]);
    let (code, k) = post(url, ADMIN, "/v1/admin/keys", serde_json::json!({"name": name, "scope": "write", "tables": {table: "write"}}));
    assert_eq!(code, 200, "{k}");
    (name, k["secret"].as_str().unwrap().to_string())
}

#[test]
fn what_an_admin_deletes_on_the_server_leaves_every_machine_that_pulled_it() {
    // WEI-303: bob left, and an admin deleted his machine's rows on the
    // server; alice, who had pulled them, kept them, and her next push
    // sent them back.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    std::env::remove_var("CHLOECTX_SYNC_ORIGIN");
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let project = "acme/ledger";
    let table = sync::table_for(project);
    let (ra, _) = machine_of(dir.path(), &url, "home-alice", ADMIN);
    let (rb, home_b) = machine_of(dir.path(), &url, "home-bob", ADMIN);
    let b = store(dir.path(), "b.strata");
    load(&b, &[note("b", Kind::Todo, "Add Redis for the job queue", 1), note("b", Kind::Fact, "The ledger runs on MySQL", 2), note("b", Kind::Decision, "Invoices are numbered per year", 3)]);
    assert_eq!(sync::push(&rb, &b, project, &mut SyncState::default()).unwrap(), 3);
    let da = dir.path().join("store-a");
    std::fs::create_dir_all(&da).unwrap();
    let a = store(&da, "memory.strata");
    load(&a, &[note("a", Kind::Note, "The ledger deploys on Tuesdays", 1)]);
    let mut sa = SyncState::default();
    assert_eq!(sync::push(&ra, &a, project, &mut sa).unwrap(), 1);
    assert_eq!(sync::pull(&ra, &a, project, &mut sa, &da).unwrap().stored, 3);

    // The admin deletes what bob's machine sent, but for his decision.
    let ob = sync::origin(&home_b);
    let (code, j) = post(&url, ADMIN, "/v1/sql", serde_json::json!({"sql": format!("DELETE FROM {table} WHERE origin = '{ob}' AND kind <> 'decision'")}));
    assert_eq!(code, 200, "{j}");
    assert_eq!(ra.count(project).unwrap(), 2);

    // alice's next pull names those she holds, noted forgotten (the
    // daemon then forgets them as `chloectx forget` does).
    let got = sync::pull(&ra, &a, project, &mut sa, &da).unwrap();
    let mut gone = got.forget.clone();
    gone.sort();
    assert_eq!((gone, got.stored), (vec!["b-00001".to_string(), "b-00002".to_string()], 0));
    assert!(sync::load_forgotten(&da).ids.contains("b-00002"));
    for id in &got.forget {
        assert!(a.delete(id).unwrap());
    }
    // Her push sends none of what she pulled back: her own records only.
    load(&a, &[note("a", Kind::Note, "The ledger's backups run at 02:00", 2)]);
    assert_eq!(sync::push(&ra, &a, project, &mut sa).unwrap(), 1, "sent what it pulled");
    assert_eq!(ra.count(project).unwrap(), 3);
    assert!(sync::pull(&ra, &a, project, &mut sa, &da).unwrap().forget.is_empty());

    // A row of her own deleted on the server stays here: hers to forget.
    let (code, j) = post(&url, ADMIN, "/v1/sql", serde_json::json!({"sql": format!("DELETE FROM {table} WHERE rid = 'a-00001'")}));
    assert_eq!(code, 200, "{j}");
    assert!(sync::pull(&ra, &a, project, &mut sa, &da).unwrap().forget.is_empty());
    assert_eq!(a.get(&["a-00001".to_string()]).unwrap().len(), 1);

    // A table made again holds no row: no delete of every record.
    let (code, j) = post(&url, ADMIN, "/v1/sql", serde_json::json!({"sql": format!("DROP TABLE {table}")}));
    assert_eq!(code, 200, "{j}");
    ra.create_table(project).unwrap();
    assert!(sync::pull(&ra, &a, project, &mut sa, &da).unwrap().forget.is_empty());
    assert_eq!(a.get(&["b-00003".to_string()]).unwrap().len(), 1, "bob's decision stays");
}

#[test]
fn a_rule_retired_on_one_machine_is_retired_on_every_machine() {
    // WEI-306: a synced rule could be retired in no one place: the command
    // center shows a synced project's memories read-only, and a rule
    // retired on one laptop stayed on every other.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    std::env::remove_var("CHLOECTX_SYNC_ORIGIN");
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let project = "acme/ledger";
    let admin = admin_remote(&dir.path().join("home-admin"), &url);
    admin.create_table(project).unwrap();
    let (bob_key, bob_secret) = sync_key(&url, project, "bob-laptop");
    let (ra, home_a) = machine_of(dir.path(), &url, "home-alice", ADMIN);
    let (rb, home_b) = machine_of(dir.path(), &url, "home-bob", &bob_secret);
    let (rc, _) = machine_of(dir.path(), &url, "home-carol", ADMIN);
    let rule = "User preference: always run make lint before a push";
    let (da, db, dc) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
    for d in [&da, &db, &dc] {
        std::fs::create_dir_all(d).unwrap();
    }
    let a = store(&da, "memory.strata");
    load(&a, &[note("a", Kind::Preference, rule, 1), note("a", Kind::Prompt, "why does lint fail?", 2)]);
    assert_eq!(sync::push(&ra, &a, project, &mut SyncState::default()).unwrap(), 2);
    let (b, c) = (store(&db, "memory.strata"), store(&dc, "memory.strata"));
    assert_eq!(sync::pull(&rb, &b, project, &mut SyncState::default(), &db).unwrap().stored, 2);
    assert_eq!(sync::pull(&rc, &c, project, &mut SyncState::default(), &dc).unwrap().stored, 2);
    sync::accept(&home_b, &b, project, &[sync::origin(&home_a)], false).unwrap();

    // bob retires alice's rule, with his key.
    let got = |s: &Store, id: &str| s.get(&[id.to_string()]).unwrap().pop().unwrap();
    let r = sync::retire(&rb, &b, project, &got(&b, "a-00001")).unwrap();
    assert_eq!(r.tag, chloectx::record::RETIRED);
    assert_eq!(got(&b, "a-00001").tag, chloectx::record::RETIRED);
    let by = |r: &Record| r.links.iter().find(|l| l.label == sync::RETIRED_BY).map(|l| l.to.clone());
    assert_eq!(by(&r).as_deref(), Some(bob_key.as_str()));
    assert!(sync::retire(&rb, &b, project, &got(&b, "a-00002")).is_err(), "a prompt is not a rule");
    let (code, j) = post(&url, ADMIN, "/v1/query", serde_json::json!({"sql": format!("SELECT tag, origin FROM {} WHERE rid = 'a-00001'", sync::table_for(project))}));
    assert_eq!(code, 200, "{j}");
    assert_eq!(j["rows"][0], serde_json::json!(["retired", sync::origin(&home_a)]), "still alice's row");

    // alice's next pull retires her own record; carol's, her copy.
    let (brief, _) = chloectx::recall::briefing(&a, "s-now", 8000, 1_800_000_000).unwrap();
    assert!(brief.contains("make lint"), "{brief}");
    for (r, s, d) in [(&ra, &a, &da), (&rc, &c, &dc)] {
        let p = sync::pull(r, s, project, &mut SyncState::default(), d).unwrap();
        assert_eq!((p.stored, p.changed), (0, 1));
        let mine = got(s, "a-00001");
        assert_eq!((mine.tag.as_str(), by(&mine)), (chloectx::record::RETIRED, Some(bob_key.clone())));
        assert_eq!(sync::pull(r, s, project, &mut SyncState::default(), d).unwrap().changed, 0);
    }
    let (brief, _) = chloectx::recall::briefing(&a, "s-now", 8000, 1_800_000_000).unwrap();
    assert!(!brief.contains("make lint"), "{brief}");
    // Retired stays retired: a row that says otherwise does not reopen it.
    let (code, _) = post(&url, ADMIN, "/v1/sql", serde_json::json!({"sql": format!("UPDATE {} SET tag = '' WHERE rid = 'a-00001'", sync::table_for(project))}));
    assert_eq!(code, 200);
    assert_eq!(sync::pull(&rc, &c, project, &mut SyncState::default(), &dc).unwrap().changed, 0);
    assert_eq!(got(&c, "a-00001").tag, chloectx::record::RETIRED);
}

#[test]
fn a_context_a_forget_cut_on_another_machine_is_cut_here() {
    // WEI-304: a forget on one machine cut the text from the context of
    // the records that kept theirs, and sent them again; a machine that
    // had pulled them kept the old context.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    std::env::remove_var("CHLOECTX_SYNC_ORIGIN");
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let project = "acme/hiring";
    let (ra, _) = machine_of(dir.path(), &url, "home-alice", ADMIN);
    let (rb, _) = machine_of(dir.path(), &url, "home-bob", ADMIN);
    let (da, db) = (dir.path().join("a"), dir.path().join("b"));
    for d in [&da, &db] {
        std::fs::create_dir_all(d).unwrap();
    }
    let a = store(&da, "memory.strata");
    let ctx = "Turn 3 request: the candidate Dana Ortiz asked for 140k\nTurn 4 request: write the offer letter";
    let mut rec = note("a", Kind::Decision, "Offer letters go out from the hiring alias", 1);
    rec.ctx = ctx.into();
    load(&a, &[rec.clone()]);
    assert_eq!(sync::push(&ra, &a, project, &mut SyncState::default()).unwrap(), 1);
    let b = store(&db, "memory.strata");
    assert_eq!(sync::pull(&rb, &b, project, &mut SyncState::default(), &db).unwrap().stored, 1);

    // alice forgets "Dana Ortiz": the decision keeps its text, and its
    // context loses the line (as the daemon's forget sends it).
    let cut = chloectx::forget::scrub_ctx(ctx, &[chloectx::forget::needle("Dana Ortiz").unwrap()]).unwrap();
    assert!(!cut.contains("Dana") && cut.contains("offer letter"), "{cut}");
    sync::push_records(&ra, project, &[Record { ctx: cut.clone(), ..rec.clone() }]).unwrap();
    let p = sync::pull(&rb, &b, project, &mut SyncState::default(), &db).unwrap();
    assert_eq!((p.stored, p.changed), (0, 1));
    let mine = b.get(&["a-00001".to_string()]).unwrap().pop().unwrap();
    assert_eq!((mine.ctx.as_str(), mine.text.as_str()), (cut.as_str(), rec.text.as_str()));

    // A row whose context says anything else changes nothing here.
    sync::push_records(&ra, project, &[Record { ctx: "Turn 3 request: something else".into(), ..rec }]).unwrap();
    assert_eq!(sync::pull(&rb, &b, project, &mut SyncState::default(), &db).unwrap().changed, 0);
    assert_eq!(b.get(&["a-00001".to_string()]).unwrap().pop().unwrap().ctx, cut);
}

/// The program, in project folder `project` with ChloeCtx's home `home`,
/// with the settings `chloectx sync setup` wrote there (no sync variables
/// in the environment), `stdin` on its stdin.
fn chloectx_here(home: &Path, project: &Path, args: &[&str], stdin: &str) -> (i32, String) {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_chloectx"))
        .args(args)
        .current_dir(project)
        .env("HOME", home)
        .env("CHLOECTX_HOME", home)
        .env("CHLOECTX_EMBEDDER", "hash:64")
        .env("CHLOECTX_DISTILL", "0")
        .env("CHLOECTX_DEEP", "off")
        .env("CHLOECTX_RECEIPTS", "0")
        .env_remove("CHLOECTX_SYNC_URL")
        .env_remove("CHLOECTX_SYNC_TOKEN")
        .env_remove("CHLOECTX_SYNC_ORIGIN")
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

#[test]
fn sync_set_up_in_one_project_uploads_no_other_projects_store() {
    // LENA-301: `chloectx sync setup` in ~/proj/ledger kept the key as the
    // machine's too, and ~/proj/shapes, never set up, pushed 55 records
    // with it, one an unredacted PyPI token.
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let home = dir.path().join("home");
    let folder = |name: &str| {
        let p = dir.path().join("proj").join(name);
        std::fs::create_dir_all(&p).unwrap();
        p.canonicalize().unwrap()
    };
    let (ledger, shapes) = (folder("ledger"), folder("shapes"));
    struct StopAll<'a>(&'a Path, Vec<&'a Path>);
    impl Drop for StopAll<'_> {
        fn drop(&mut self) {
            for p in &self.1 {
                let _ = chloectx_here(self.0, p, &["stop"], "");
            }
        }
    }
    let _stop = StopAll(&home, vec![&ledger, &shapes]);
    let (ledger_key, shapes_key) = (sync::project_key(&home, &ledger.to_string_lossy()), sync::project_key(&home, &shapes.to_string_lossy()));
    let server = |sql: String| post(&url, ADMIN, "/v1/query", serde_json::json!({"sql": sql}));

    let (code, out) = chloectx_here(&home, &ledger, &["sync", "setup", "--url", &url, "--token-stdin"], ADMIN);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("sync.token"), "the key is the project's only: {out}");
    assert!(out.contains(&format!("only {ledger_key} syncs")), "{out}");
    assert!(!sync::token_path(&home).exists());

    // The other project records a secret, and its daemon starts, pushes as
    // it would every 30 s, and exits, as it pushes too.
    let token = format!("pypi-AgEIcHlwaS5vcmc{}", "aB3dE5gH7jK9mN1pQ2sT4vW6yZ8".repeat(3));
    let (code, out) = chloectx_here(&home, &shapes, &["remember", "fact", &format!("shape pypi: {token}")], "");
    assert_eq!(code, 0, "{out}");
    let (code, out) = chloectx_here(&home, &shapes, &["sync", "status"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\"syncs\": false") && out.contains("does not sync") && out.contains(&ledger_key), "{out}");
    let (code, out) = chloectx_here(&home, &shapes, &["sync", "push"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("does not sync"), "{out}");
    let (code, out) = chloectx_here(&home, &shapes, &["stop"], "");
    assert_eq!(code, 0, "{out}");
    let (code, out) = chloectx_here(&home, &shapes, &["doctor"], "");
    assert!(out.contains(&format!("this project ({shapes_key}) does not sync")) && out.contains(&format!("only {ledger_key} syncs")), "{code}: {out}");
    let (code, j) = server(format!("SELECT COUNT(*) FROM {}", sync::table_for(&shapes_key)));
    assert!(code != 200 && j.to_string().contains("no such table"), "shapes reached the server: {code} {j}");

    // The project set up syncs, with its key.
    let (code, out) = chloectx_here(&home, &ledger, &["remember", "fact", "The ledger closes on the 3rd"], "");
    assert_eq!(code, 0, "{out}");
    let (code, out) = chloectx_here(&home, &ledger, &["sync", "status"], "");
    assert!(code == 0 && out.contains("\"syncs\": true") && out.contains("the project's key"), "{out}");
    let (code, out) = chloectx_here(&home, &ledger, &["sync", "push"], "");
    assert_eq!(code, 0, "{out}");
    let (code, j) = server(format!("SELECT content FROM {}", sync::table_for(&ledger_key)));
    assert!(code == 200 && j.to_string().contains("closes on the 3rd"), "{code} {j}");

    // --every-project, asked for: now the other one pushes too, redacted.
    let (code, out) = chloectx_here(&home, &ledger, &["sync", "setup", "--url", &url, "--token-stdin", "--every-project", "--no-create"], ADMIN);
    assert!(code == 0 && out.contains("every project on this machine syncs"), "{out}");
    let (code, out) = chloectx_here(&home, &shapes, &["sync", "push"], "");
    assert!(code == 0 && out.contains("\"pushed\": 1"), "{out}");
    let (code, j) = server(format!("SELECT content FROM {}", sync::table_for(&shapes_key)));
    assert!(code == 200 && j.to_string().contains("[REDACTED:pypi-token]") && !j.to_string().contains(&token), "{code} {j}");

    // And `sync off --every-project` takes it back; the server keeps what it was sent.
    let (code, out) = chloectx_here(&home, &ledger, &["sync", "off", "--every-project"], "");
    assert!(code == 0 && out.contains("no longer syncs every project"), "{out}");
    let (_, out) = chloectx_here(&home, &shapes, &["sync", "status"], "");
    assert!(out.contains("\"syncs\": false") && out.contains("sent 1 record to"), "{out}");
}

/// ANA-401: two machines that pulled the same records each keep a note in a
/// terminal; the notes have ids of their own, so both reach the server and
/// the other machine. A store with the old, alike ids is told by `clashing`.
#[test]
fn notes_kept_in_a_terminal_on_two_machines_both_arrive() {
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    std::env::remove_var("CHLOECTX_SYNC_ORIGIN");
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), None);
    let machine = |name: &str| {
        let home = dir.path().join(name);
        sync::setup(&home, &dir.path().to_string_lossy(), &url, None, "", &[], false).unwrap();
        sync::remote(&home).unwrap()
    };
    let (ra, rb) = (machine("home-a"), machine("home-b"));
    let project = "acme/ledger";
    let (a, b) = (store(dir.path(), "a.strata"), store(dir.path(), "b.strata"));
    let (mut sa, mut sb) = (SyncState::default(), SyncState::default());
    load(&a, &records("s", 1..=5));
    sync::push(&ra, &a, project, &mut sa).unwrap();
    sync::pull(&rb, &b, project, &mut sb, dir.path()).unwrap();
    assert_eq!(b.len(), 5);
    let kept = |s: &Store, text: &str| {
        let d = chloectx::record::Draft::new(Kind::Fact, text.to_string());
        s.append("manual", 0, 1_700_000_100_000, vec![d]).unwrap().pop().unwrap()
    };
    let (na, nb) = (kept(&a, "Staging resets every night at 02:00"), kept(&b, "The queue workers run under systemd"));
    assert_ne!(na.id, nb.id, "numbered alike: {}", na.id);
    sync::push(&ra, &a, project, &mut sa).unwrap();
    sync::push(&rb, &b, project, &mut sb).unwrap();
    assert_eq!(ra.count(project).unwrap(), 7);
    sync::pull(&ra, &a, project, &mut sa, dir.path()).unwrap();
    sync::pull(&rb, &b, project, &mut sb, dir.path()).unwrap();
    assert!(a.get(&[nb.id.clone()]).unwrap().len() == 1 && b.get(&[na.id.clone()]).unwrap().len() == 1);
    assert!(ra.clashing(&a, project, 100).unwrap().is_empty());

    // A store from before: its note and another machine's share an id.
    let old = store(dir.path(), "old.strata");
    let mut r = note("manual", Kind::Fact, "An old note that took the id", 6);
    r.id = na.id.clone();
    load(&old, &[r]);
    let home = dir.path().join("home-old");
    sync::setup(&home, &dir.path().to_string_lossy(), &url, None, "", &[], false).unwrap();
    let ro = sync::remote(&home).unwrap();
    assert_eq!(ro.clashing(&old, project, 100).unwrap(), vec![na.id]);
}

/// WEI-502: a new hire's script failed to make their key, so the setup ran
/// with nothing on stdin. That is refused; a setup without a key that the
/// server answers 401 fails; and doctor does not say "ready" for it.
#[test]
fn a_setup_the_server_refuses_is_not_called_ready() {
    let Some(bin) = server_bin() else {
        eprintln!("skipped: no chloedb-server binary (set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (_server, url) = serve(&bin, dir.path(), Some(ADMIN));
    let home = dir.path().join("home");
    let p = dir.path().join("proj").join("payments");
    std::fs::create_dir_all(&p).unwrap();
    let p = p.canonicalize().unwrap();
    struct Stop<'a>(&'a Path, &'a Path);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            let _ = chloectx_here(self.0, self.1, &["stop"], "");
        }
    }
    let _stop = Stop(&home, &p);

    let (code, out) = chloectx_here(&home, &p, &["sync", "setup", "--url", &url, "--token-stdin", "--no-create"], "");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("--token-stdin read no key"), "{out}");
    let (code, out) = chloectx_here(&home, &p, &["sync", "setup", "--url", &url, "--no-create"], "");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("refuses this project's sync") && out.contains(": 401:"), "{out}");
    let (code, out) = chloectx_here(&home, &p, &["doctor"], "");
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("FAIL  sync") && out.contains("the server refuses it"), "{out}");
    assert!(!out.contains("ChloeCtx is ready"), "{out}");
}
