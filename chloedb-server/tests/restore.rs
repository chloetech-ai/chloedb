//! Restoring a sharded, replicated cluster from its checkpoints, with
//! server processes as an operator runs them on machines of their own
//! (findings OLU-2 and OLU-8 of the persona study of 17 November 2026).
//! A cluster of 2 shards of 3 copies and a coordinator, its slots moved so
//! that its map is not the one a new cluster makes, an API key, and a
//! checkpoint of every shard through the coordinator, which writes each
//! shard's cluster map and keys beside its checkpoint. Then: a copy whose
//! disk was wiped gets the cluster map back from the coordinator; a whole
//! shard lost while the rest serves is restored from its checkpoint with
//! `chloedb-server restore`, its other copies started on empty disks; and
//! the whole cluster, every disk lost, is restored from the checkpoints
//! copied off its machines: the rows as they were at the checkpoint, the
//! map, the key, and writes taken again.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

const CLUSTER: &str = "restore-cluster-secret";
const ADMIN: &str = "restore-admin-token";
const SCHEMA: &str = r#"{"table": "acks", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]}"#;
const WAIT: Duration = Duration::from_secs(60);

/// A port free now, from a block this test process starts at (by its
/// process id), as tests/failover.rs takes them: a server started again
/// binds the same ports.
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = (std::process::id() % 500) * 40 + 7;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 400, "this test process tried 400 ports and found none free");
        let p = (10_000 + (base + n) % 20_000) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_chloedb-server")
}

async fn http(base: &str, method: &str, path: &str, auth: &str, body: Option<Json>) -> (u16, Json) {
    let c = chloedb_server::http_client().timeout(Duration::from_secs(30)).build().unwrap();
    let mut r = match method {
        "GET" => c.get(format!("{base}{path}")),
        _ => c.post(format!("{base}{path}")),
    };
    r = r.bearer_auth(auth);
    if let Some(b) = body {
        r = r.json(&b);
    }
    match r.send().await {
        Ok(r) => (r.status().as_u16(), r.json().await.unwrap_or(Json::Null)),
        Err(e) => (0, json!({"error": e.to_string()})),
    }
}

/// The rows of an answer, each as its values.
fn rows(a: &Json) -> Vec<Vec<Json>> {
    a["rows"].as_array().into_iter().flatten().map(|r| r.as_array().cloned().unwrap_or_else(|| r.as_object().map(|o| o.values().cloned().collect()).unwrap_or_default())).collect()
}

/// One copy of a shard: a `chloedb-server` process of its database, on
/// ports it keeps, in a folder of its own (its disk) beside a backup
/// folder of its own (another disk of its machine).
struct Copy {
    name: String,
    disk: PathBuf,
    backup: PathBuf,
    log: PathBuf,
    http: u16,
    grpc: u16,
    /// Its part in replication: `--replicas ...` or `--replica`.
    role: Vec<String>,
    child: Option<Child>,
}

impl Copy {
    fn new(dir: &Path, name: &str) -> Copy {
        let disk = dir.join("disks").join(name);
        std::fs::create_dir_all(&disk).unwrap();
        Copy { name: name.into(), backup: dir.join("backups").join(name), disk, log: dir.join(format!("{name}.log")), http: port(), grpc: port(), role: Vec::new(), child: None }
    }

    fn db(&self) -> PathBuf {
        self.disk.join("acks.chloedb")
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    async fn start(&mut self) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.log).unwrap();
        let mut cmd = Command::new(bin());
        cmd.arg("--db").arg(self.db()).args(["--listen", &format!("127.0.0.1:{}", self.http), "--grpc", &format!("127.0.0.1:{}", self.grpc)]);
        cmd.args(["--seq-no", "--log", "info", "--failover-after-ms", "1500", "--cluster-token", CLUSTER, "--start-apps", "off"]);
        cmd.arg("--backup-dir").arg(&self.backup).args(&self.role);
        if !self.db().exists() {
            cmd.args(["--schema", SCHEMA]);
        }
        self.child = Some(cmd.stdout(Stdio::null()).stderr(log).spawn().unwrap());
        let t0 = Instant::now();
        while self.health().await.is_none() {
            assert!(t0.elapsed() < WAIT, "{} did not start: {}", self.name, self.logs());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Its disk lost: every file of its database gone.
    fn wipe(&mut self) {
        self.stop();
        std::fs::remove_dir_all(&self.disk).unwrap();
        std::fs::create_dir_all(&self.disk).unwrap();
    }

    async fn health(&self) -> Option<Json> {
        let (s, j) = http(&self.base(), "GET", "/v1/health", CLUSTER, None).await;
        (s == 200).then_some(j)
    }

    /// Its ids, read from it alone.
    async fn ids(&self) -> BTreeSet<String> {
        let (s, a) = http(&self.base(), "POST", "/v1/query", CLUSTER, Some(json!({"sql": "SELECT id FROM acks LIMIT 10000"}))).await;
        assert_eq!(s, 200, "{}: {a}", self.name);
        rows(&a).into_iter().map(|r| r[0].as_str().unwrap().to_string()).collect()
    }

    fn logs(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Runs `chloedb-server restore --from FROM --db DB [more]`: whether it
/// succeeded, and what it printed.
fn restore(from: &Path, db: &Path, more: &[&str]) -> (bool, String) {
    let out = Command::new(bin()).arg("restore").arg("--from").arg(from).arg("--db").arg(db).args(more).output().unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// Two shards of three copies each and a coordinator.
struct Cluster {
    dir: PathBuf,
    shards: Vec<Vec<Copy>>,
    http: u16,
    coordinator: Option<Child>,
}

impl Cluster {
    async fn start(dir: &Path) -> Cluster {
        let mut shards: Vec<Vec<Copy>> = (0..2).map(|i| (0..3).map(|k| Copy::new(dir, &format!("s{i}{}", ['a', 'b', 'c'][k]))).collect()).collect();
        for shard in &mut shards {
            let others: Vec<String> = shard[1..].iter().map(Copy::url).collect();
            shard[0].role = vec!["--replicas".into(), others.join(",")];
            for c in &mut shard[1..] {
                c.role = vec!["--replica".into()];
            }
        }
        let mut c = Cluster { dir: dir.to_path_buf(), shards, http: port(), coordinator: None };
        c.start_copies().await;
        c.start_coordinator().await;
        c
    }

    async fn start_copies(&mut self) {
        for c in self.shards.iter_mut().flatten() {
            if c.child.is_none() {
                c.start().await;
            }
        }
    }

    async fn start_coordinator(&mut self) {
        let list = self.shards.iter().map(|s| s.iter().map(Copy::url).collect::<Vec<_>>().join("|")).collect::<Vec<_>>().join(",");
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("coordinator.log")).unwrap();
        let child = Command::new(bin())
            .args(["--shards", &list, "--listen", &format!("127.0.0.1:{}", self.http), "--grpc", "off", "--log", "info", "--token", ADMIN, "--cluster-token", CLUSTER, "--start-apps", "off"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.coordinator = Some(child);
        self.until("the coordinator serves", || async { self.call("GET", "/v1/cluster", None).await.0 == 200 }).await;
    }

    fn stop_coordinator(&mut self) {
        if let Some(mut c) = self.coordinator.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    async fn call(&self, method: &str, path: &str, body: Option<Json>) -> (u16, Json) {
        http(&self.base(), method, path, ADMIN, body).await
    }

    async fn sql(&self, sql: &str) -> Json {
        let (s, a) = self.call("POST", "/v1/sql", Some(json!({"sql": sql}))).await;
        assert_eq!(s, 200, "{sql}: {a}");
        a
    }

    async fn ids(&self) -> BTreeSet<String> {
        rows(&self.sql("SELECT id FROM acks LIMIT 10000").await).into_iter().map(|r| r[0].as_str().unwrap().to_string()).collect()
    }

    async fn insert(&self, from: usize, to: usize) {
        let values: Vec<String> = (from..to).map(|i| format!("('r{i:04}', {i})")).collect();
        self.sql(&format!("INSERT INTO acks (id, n) VALUES {}", values.join(", "))).await;
    }

    async fn epoch(&self) -> u64 {
        self.call("GET", "/v1/cluster", None).await.1["slot_epoch"].as_u64().unwrap_or(0)
    }

    fn logs(&self) -> String {
        let mut out = format!("--- coordinator\n{}", std::fs::read_to_string(self.dir.join("coordinator.log")).unwrap_or_default());
        for c in self.shards.iter().flatten() {
            out.push_str(&format!("\n--- {}\n{}", c.name, c.logs()));
        }
        out
    }

    async fn until<F: std::future::Future<Output = bool>>(&self, what: &str, f: impl Fn() -> F) {
        let t0 = Instant::now();
        while !f().await {
            if t0.elapsed() > WAIT {
                panic!("waited {} s for: {what}\n{}", WAIT.as_secs(), self.logs());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Every copy of shard `i` answers, holds the same last transaction,
    /// holds the cluster map of epoch `epoch`, and serves reads: a replica
    /// that installed a copy of its primary serves none until the next call
    /// of its primary finds it caught up, which comes late on a busy host
    /// (a primary's health has no `serves_reads`).
    async fn shard_whole(&self, i: usize, epoch: u64) -> bool {
        let mut seqs = BTreeSet::new();
        for c in &self.shards[i] {
            let Some(h) = c.health().await else { return false };
            if h["slots"]["epoch"].as_u64() != Some(epoch) || h["replication"]["serves_reads"] == false {
                return false;
            }
            seqs.insert(h["replication"]["seq_no"].as_u64().unwrap_or(0));
        }
        let primary = self.shards[i][0].health().await.unwrap_or(Json::Null);
        let in_sync = primary["replication"]["replicas"].as_array().is_some_and(|r| r.len() == 2 && r.iter().all(|r| r["in_sync"] == true));
        seqs.len() == 1 && in_sync
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        self.stop_coordinator();
    }
}

/// Moves slots [0, 512) of shard 0 to shard 1, and waits for the move.
async fn move_slots(c: &Cluster) -> u64 {
    let (s, j) = c.call("POST", "/v1/admin/moves", Some(json!({"slots": [[0, 512]], "to": 1}))).await;
    assert!(s == 200 || s == 202, "{s}: {j}");
    c.until("the slots moved", || async {
        let (_, j) = c.call("GET", "/v1/admin/moves", None).await;
        j["running"] == false && j["moving"].is_null() && j["state"] == "done"
    })
    .await;
    c.epoch().await
}

/// A copy whose disk was lost, started again empty with the same flags,
/// is sent a copy of its primary's rows, and the coordinator gives it the
/// cluster map (OLU-8: it never got one, and refused every read routed by
/// the map while `/v1/cluster` said it was ok).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_copy_rebuilt_on_an_empty_disk_gets_the_cluster_map_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Cluster::start(dir.path()).await;
    c.insert(0, 200).await;
    let epoch = move_slots(&c).await;
    assert!(epoch >= 2, "{epoch}");
    c.until("shard 1 is whole", || c.shard_whole(1, epoch)).await;

    c.shards[1][2].wipe();
    c.shards[1][2].start().await;
    c.until("the rebuilt copy holds the primary's rows and the cluster map", || c.shard_whole(1, epoch)).await;
    let rebuilt = c.shards[1][2].health().await.unwrap();
    assert_eq!(rebuilt["slots"]["shard"], 1, "{rebuilt}");
    // It answers a read routed by the map, as its primary does.
    assert_eq!(c.shards[1][2].ids().await, c.shards[1][0].ids().await);
    // `/v1/cluster` compares copies by replication's numbers and says the map each holds.
    let (_, cl) = c.call("GET", "/v1/cluster", None).await;
    let copies = cl["shards"][1]["replicas"].as_array().unwrap();
    assert!(copies.iter().all(|r| r["slot_epoch"] == epoch), "{cl}");
    assert_eq!(copies.iter().map(|r| r["seq_no"].as_u64()).collect::<BTreeSet<_>>().len(), 1, "{cl}");
    assert!(c.logs().contains("gave the cluster map to a copy that held an older one, or none"));
}

/// A checkpoint of every shard through the coordinator carries each
/// shard's cluster map and the cluster's keys; a shard whose three disks
/// were lost is restored from its checkpoint while the other shard
/// serves; and the whole cluster, every disk lost, is restored from the
/// checkpoints copied off its machines (OLU-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_is_restored_from_its_checkpoints() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut c = Cluster::start(d).await;
    c.insert(0, 300).await;
    let epoch = move_slots(&c).await;
    let (s, key) = c.call("POST", "/v1/admin/keys", Some(json!({"name": "dashboards", "scope": "read"}))).await;
    assert_eq!(s, 200, "{key}");
    let secret = key["secret"].as_str().unwrap().to_string();
    // The key reaches the shards, whose keys files a checkpoint copies.
    c.until("the key reaches every copy", || async {
        let mut all = true;
        for k in c.shards.iter().flatten() {
            all &= std::fs::read_to_string(chloedb_server::keys::Keys::file_of(&k.db())).is_ok_and(|t| t.contains("dashboards"));
        }
        all
    })
    .await;
    for i in 0..2 {
        c.until("the shard is whole", || c.shard_whole(i, epoch)).await;
    }
    let at_checkpoint = c.ids().await;
    assert_eq!(at_checkpoint.len(), 300);
    let shard1: BTreeSet<String> = c.shards[1][0].ids().await;
    assert!(!shard1.is_empty() && shard1.len() < 300);

    // One call checkpoints every shard, each on its primary, with its map and keys beside it.
    let (s, ck) = c.call("POST", "/v1/admin/checkpoint", Some(json!({"name": "nightly"}))).await;
    assert_eq!(s, 200, "{ck}");
    assert_eq!(ck["slot_epoch"], epoch, "{ck}");
    let shards = ck["shards"].as_array().unwrap();
    assert_eq!(shards.len(), 2, "{ck}");
    for (i, s) in shards.iter().enumerate() {
        assert_eq!(s["url"], c.shards[i][0].url(), "{ck}");
        assert_eq!(s["checkpoint"]["files"], json!(["nightly", "nightly.slots", "nightly.keys.json"]), "{ck}");
        assert_eq!(s["checkpoint"]["slots"], json!({"shard": i, "epoch": epoch, "shards": 2}), "{ck}");
        let kept = chloedb_server::restore::KeptMap::read(&c.shards[i][0].backup.join("nightly.slots")).unwrap().unwrap();
        assert_eq!((kept.shard, kept.map.epoch), (i, epoch));
        assert!(std::fs::read_to_string(c.shards[i][0].backup.join("nightly.keys.json")).unwrap().contains("dashboards"));
    }
    // A name the server refuses is refused before any shard writes.
    let (s, bad) = c.call("POST", "/v1/admin/checkpoint", Some(json!({"name": "127.0.0.1:7701-x"}))).await;
    assert_eq!(s, 400, "{bad}");

    // The accident, and writes after the checkpoint, which a restore loses.
    c.sql("DELETE FROM acks WHERE n < 100").await;
    c.insert(300, 350).await;
    assert_eq!(c.ids().await.len(), 250);

    // A copy's restore refuses while its server runs, and a checkpoint of another shard.
    let (ok, said) = restore(&c.shards[1][0].backup.join("nightly"), &c.shards[1][0].db(), &[]);
    assert!(!ok && said.contains("open: stop it first"), "{said}");
    c.shards[0][1].stop();
    let (ok, said) = restore(&c.shards[1][0].backup.join("nightly"), &c.shards[0][1].db(), &[]);
    assert!(!ok && said.contains("the checkpoint is of shard 1"), "{said}");
    c.shards[0][1].start().await;

    // Shard 1's three disks are lost. Its primary is restored from the
    // checkpoint (on its backup disk), and its other copies start empty.
    for k in 0..3 {
        c.shards[1][k].wipe();
    }
    let (ok, said) = restore(&c.shards[1][0].backup.join("nightly"), &c.shards[1][0].db(), &[]);
    assert!(ok, "{said}");
    assert!(said.contains(&format!("shard 1 of 2, epoch {epoch}")), "{said}");
    c.shards[1][0].start().await;
    c.shards[1][1].start().await;
    c.shards[1][2].start().await;
    c.until("shard 1 is whole again", || c.shard_whole(1, epoch)).await;
    assert_eq!(c.shards[1][0].ids().await, shard1, "shard 1 holds its rows of the checkpoint");
    assert_eq!(c.shards[1][2].ids().await, shard1);
    let now: BTreeSet<String> = c.ids().await;
    let shard0_now: BTreeSet<String> = c.shards[0][0].ids().await;
    assert_eq!(now, shard0_now.union(&shard1).cloned().collect::<BTreeSet<_>>());
    c.insert(400, 410).await;

    // Every disk is lost, the backup disks too: only the checkpoints copied
    // off the machines are left. Each copy is restored from its shard's.
    let offsite = d.join("offsite");
    for (i, shard) in c.shards.iter().enumerate() {
        let to = offsite.join(format!("s{i}"));
        std::fs::create_dir_all(&to).unwrap();
        for f in ["nightly", "nightly.slots", "nightly.keys.json"] {
            std::fs::copy(shard[0].backup.join(f), to.join(f)).unwrap();
        }
    }
    c.stop_coordinator();
    for k in c.shards.iter_mut().flatten() {
        k.wipe();
        std::fs::remove_dir_all(&k.backup).unwrap();
    }
    for (i, shard) in c.shards.iter().enumerate() {
        for k in shard {
            let (ok, said) = restore(&offsite.join(format!("s{i}")).join("nightly"), &k.db(), &[]);
            assert!(ok, "{}: {said}", k.name);
            assert!(said.contains("keys merged into") && !said.contains("before-restore"), "{said}");
        }
    }
    c.start_copies().await;
    c.start_coordinator().await;
    for i in 0..2 {
        c.until("the shard is whole after the restore", || c.shard_whole(i, epoch)).await;
    }
    assert_eq!(c.epoch().await, epoch, "the coordinator routes by the map kept beside the checkpoints");
    assert_eq!(c.ids().await, at_checkpoint, "the rows as they were at the checkpoint");
    // The key made before the checkpoint works again.
    let (s, a) = http(&c.base(), "POST", "/v1/query", &secret, Some(json!({"sql": "SELECT COUNT(*) FROM acks"}))).await;
    assert_eq!(s, 200, "{a}");
    // Writes are taken, by every copy.
    c.insert(500, 520).await;
    assert_eq!(c.ids().await.len(), 320);
    for i in 0..2 {
        c.until("the shard is whole after new writes", || c.shard_whole(i, epoch)).await;
    }
}
