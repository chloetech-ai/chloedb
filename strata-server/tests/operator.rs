//! The operator's steps ([`strata_server::operator::reconcile`]) on a
//! platform of server processes on this machine, which stand for a
//! Kubernetes cluster's pods: each set of copies is a StatefulSet's pods,
//! each with a directory for its volume, stopped as the kubelet stops a
//! pod (SIGTERM, then a kill after the grace period); the coordinators a
//! Deployment that starts a new one, ready, before it stops an old one,
//! behind a Service (a TCP proxy sending each connection to a coordinator
//! whose `/readyz` answers). A cluster is made, grows and shrinks in
//! shards and in copies, hands a primary over before its copy goes, starts
//! every copy again with new settings one at a time, and takes back a
//! copy that lost its volume, answering every row throughout: open, and
//! with a cluster token of its own, the admin token held by the
//! coordinator alone. And a new image rolls out while queries and writes
//! come in, none of them failing; one whose servers exit is rolled back,
//! and one that does not run is refused before any server restarts.

#![cfg(unix)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata_server::operator::{back_to, noted, patched, reconcile, rollback_target, versions, Admin, Check, Gates, OnFailure, Outcome, Phase, Platform, Rolled, Rollout, RolloutSpec, Set, Spec, Trouble};
use strata_server::coord::IN_DOUBT;

const ROWS: usize = 300;

/// How long a server serves on once told to stop (`--drain-ms`).
const DRAIN_MS: u64 = 1000;

/// A port of a block of 200 this process takes alone (as in
/// tests/failover.rs, larger: its tests run at once, and a rolling
/// upgrade starts new coordinators beside the old). The blocks stay under
/// 32768, where Linux starts the ports it gives outgoing connections: a
/// port found free here is bound later (a coordinator's Service, a server
/// started again), and a connection could take it meanwhile.
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = 10_000 + (std::process::id() % 113) * 200;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 200, "this test process took its 200 ports");
        let p = (base + n) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

/// The binary an image runs, and arguments it adds: an image named
/// `:missing` has none (it cannot be pulled), and one named `:crashes`
/// runs a server that exits at once.
fn binary(image: &str) -> Option<(PathBuf, Vec<String>)> {
    match image.rsplit(':').next() {
        Some("missing") => None,
        Some("crashes") => Some((PathBuf::from(env!("CARGO_BIN_EXE_strata-server")), vec!["--no-such-flag".into()])),
        _ => Some((PathBuf::from(env!("CARGO_BIN_EXE_strata-server")), Vec::new())),
    }
}

struct Pod {
    /// None when its image could not be started.
    child: Option<Child>,
    http: u16,
    /// The settings it was started with ([`Spec::server_config`], or a
    /// coordinator's).
    config: String,
    image: String,
    /// Its log, for why it exited.
    log: PathBuf,
}

impl Pod {
    /// Why it does not run, if it does not: exited, or never started.
    fn trouble(&mut self) -> Option<String> {
        let Some(c) = self.child.as_mut() else { return Some(format!("ErrImagePull: the image {} cannot be pulled", self.image)) };
        let status = c.try_wait().ok().flatten()?;
        let log = std::fs::read_to_string(&self.log).unwrap_or_default();
        let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string();
        Some(format!("it {status}: {last}"))
    }

    /// Asks it to stop, as the kubelet does (SIGTERM).
    fn terminate(&self) {
        if let Some(c) = &self.child {
            unsafe { libc::kill(c.id() as i32, libc::SIGTERM) };
        }
    }

    /// Whether it has stopped, killing it once the grace period is over.
    fn stopped(&mut self, since: Instant) -> bool {
        let Some(c) = self.child.as_mut() else { return true };
        if c.try_wait().ok().flatten().is_some() {
            return true;
        }
        if since.elapsed() > Duration::from_secs(20) {
            let _ = c.kill();
            let _ = c.wait();
            return true;
        }
        false
    }
}

impl Drop for Pod {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// A coordinator pod of the Deployment, and the shards list it runs.
struct CoordPod {
    pod: Pod,
    list: Vec<Vec<String>>,
}

/// The coordinators' Deployment: what its template asks, and its pods.
#[derive(Default)]
struct Deployment {
    want: Option<(Vec<Vec<String>>, usize, Spec)>,
    pods: Vec<CoordPod>,
}

/// The coordinators' Service: a TCP proxy sending each new connection to
/// a coordinator whose `/readyz` answered 200 within the last 100 ms, as
/// endpoints follow readiness.
struct Service {
    /// Every coordinator process that runs, ready or not.
    ports: Mutex<Vec<u16>>,
    ready: Mutex<Vec<u16>>,
    next: AtomicUsize,
}

impl Service {
    async fn start(port: u16) -> Arc<Service> {
        let svc = Arc::new(Service { ports: Mutex::default(), ready: Mutex::default(), next: AtomicUsize::new(0) });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        let s = svc.clone();
        tokio::spawn(async move {
            let http = strata_server::http_client().timeout(Duration::from_millis(500)).build().unwrap();
            loop {
                let ports = s.ports.lock().unwrap().clone();
                let mut ready = Vec::new();
                for p in ports {
                    if http.get(format!("http://127.0.0.1:{p}/readyz")).send().await.is_ok_and(|r| r.status().is_success()) {
                        ready.push(p);
                    }
                }
                *s.ready.lock().unwrap() = ready;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        let s = svc.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut client, _)) = listener.accept().await else { continue };
                let s = s.clone();
                tokio::spawn(async move {
                    // No coordinator ready: the connection waits a little,
                    // then is refused.
                    let t0 = Instant::now();
                    let to = loop {
                        let ready = s.ready.lock().unwrap().clone();
                        if !ready.is_empty() {
                            break Some(ready[s.next.fetch_add(1, Ordering::Relaxed) % ready.len()]);
                        }
                        if t0.elapsed() > Duration::from_secs(2) {
                            break None;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    };
                    let Some(to) = to else { return };
                    if let Ok(mut server) = tokio::net::TcpStream::connect(("127.0.0.1", to)).await {
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                    }
                });
            }
        });
        svc
    }
}

#[derive(Default)]
struct State {
    /// Each set's demo shard, and its pods by ordinal.
    sets: BTreeMap<u32, (Option<(usize, usize)>, BTreeMap<usize, Pod>)>,
    /// The settings each set starts its copies with, as its StatefulSet's
    /// pod template holds them.
    configs: BTreeMap<u32, Spec>,
    ports: HashMap<(u32, usize), (u16, u16)>,
    /// Copies told to stop, each started again once it has (the
    /// StatefulSet makes the pod again).
    stopping: Vec<(u32, usize, Pod, Instant)>,
    deployment: Deployment,
    /// Coordinators told to stop.
    retiring: Vec<(Pod, Instant)>,
}

struct Procs {
    dir: PathBuf,
    schema: PathBuf,
    coordinator_port: u16,
    /// The admin token and the cluster token, as `tokenSecret` and
    /// `clusterTokenSecret` give them: the servers take the cluster token,
    /// the coordinator both.
    tokens: Option<(String, String)>,
    /// The spec the operator works to, as the `StrataCluster` the
    /// platform's objects are made from.
    spec: Mutex<Spec>,
    state: Mutex<State>,
    service: tokio::sync::OnceCell<Arc<Service>>,
    /// Images tried before a rollout, and copies restarted.
    preflights: Mutex<Vec<String>>,
    restarts: AtomicUsize,
}

impl Procs {
    fn new(dir: &Path, tokens: Option<(&str, &str)>) -> Procs {
        let schema = dir.join("schema.json");
        std::fs::write(&schema, serde_json::to_string(&strata_server::demo::schema(ROWS)).unwrap()).unwrap();
        let tokens = tokens.map(|(a, c)| (a.to_string(), c.to_string()));
        Procs {
            dir: dir.to_path_buf(),
            schema,
            coordinator_port: port(),
            tokens,
            spec: Mutex::default(),
            state: Mutex::new(State::default()),
            service: tokio::sync::OnceCell::new(),
            preflights: Mutex::default(),
            restarts: AtomicUsize::new(0),
        }
    }

    fn ports(&self, id: u32, k: usize) -> (u16, u16) {
        *self.state.lock().unwrap().ports.entry((id, k)).or_insert_with(|| (port(), port()))
    }

    fn volume(&self, id: u32, k: usize) -> PathBuf {
        self.dir.join(format!("s{id}-{k}"))
    }

    fn log(&self, name: &str) -> (std::fs::File, PathBuf) {
        let path = self.dir.join(format!("{name}.log"));
        (std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap(), path)
    }

    /// Starts copy `k` of set `id` as the StatefulSet's script does, with
    /// its set's settings.
    fn start(&self, id: u32, k: usize, demo: Option<(usize, usize)>) -> Pod {
        let set = self.state.lock().unwrap().configs.get(&id).cloned();
        let set = set.unwrap_or_else(|| self.spec.lock().unwrap().clone());
        let (http, grpc) = self.ports(id, k);
        let vol = self.volume(id, k);
        std::fs::create_dir_all(&vol).unwrap();
        let db = vol.join("db.strata");
        let (log, log_path) = self.log(&format!("s{id}-{k}"));
        let Some((bin, extra)) = binary(&set.image) else {
            return Pod { child: None, http, config: set.server_config(), image: set.image.clone(), log: log_path };
        };
        let mut cmd = Command::new(bin);
        cmd.arg("--db").arg(&db).args(["--seq-no", "--listen", &format!("127.0.0.1:{http}"), "--grpc", &format!("127.0.0.1:{grpc}")]);
        cmd.args(["--failover-after-ms", &set.failover_after_ms.to_string(), "--log", "info", "--drain-ms", &DRAIN_MS.to_string()]);
        cmd.args(&set.args).args(&extra);
        if let Some((_, cluster)) = &self.tokens {
            cmd.args(["--cluster-token", cluster]);
        }
        if !db.exists() {
            match demo {
                Some((i, n)) if k == 0 => cmd.args(["--demo", &ROWS.to_string(), "--demo-shard", &format!("{i}/{n}")]),
                _ => cmd.arg("--schema").arg(&self.schema),
            };
        }
        let child = cmd.stdout(Stdio::null()).stderr(log).spawn().unwrap();
        Pod { child: Some(child), http, config: set.server_config(), image: set.image.clone(), log: log_path }
    }

    /// Starts a coordinator over `list` with `spec`'s settings, on a port
    /// of its own.
    fn start_coordinator(&self, list: &[Vec<String>], spec: &Spec) -> CoordPod {
        let http = port();
        let (log, log_path) = self.log(&format!("coordinator-{http}"));
        let config = spec.coordinator_config();
        let Some((bin, extra)) = binary(&spec.image) else {
            return CoordPod { pod: Pod { child: None, http, config, image: spec.image.clone(), log: log_path }, list: list.to_vec() };
        };
        let shards = list.iter().map(|s| s.join("|")).collect::<Vec<_>>().join(",");
        let mut cmd = Command::new(bin);
        cmd.args(["--shards", &shards, "--listen", &format!("127.0.0.1:{http}"), "--grpc", "off", "--log", "info", "--drain-ms", &DRAIN_MS.to_string()]);
        cmd.args(&spec.coordinator_args).args(&extra);
        if let Some((admin, cluster)) = &self.tokens {
            cmd.args(["--token", admin, "--cluster-token", cluster]);
        }
        let child = cmd.stdout(Stdio::null()).stderr(log).spawn().unwrap();
        CoordPod { pod: Pod { child: Some(child), http, config, image: spec.image.clone(), log: log_path }, list: list.to_vec() }
    }

    /// What the StatefulSets' and the Deployment's controllers do between
    /// two looks: a copy that stopped starts again, and the Deployment
    /// moves its pods toward its template, one at a time: a new one
    /// starts (one more than asked at most), and an old one stops once
    /// as many as asked are ready without it.
    fn drive(&self) {
        let ready: Vec<u16> = self.service.get().map(|s| s.ready.lock().unwrap().clone()).unwrap_or_default();
        let mut restart = Vec::new();
        {
            let mut st = self.state.lock().unwrap();
            let mut i = 0;
            while i < st.stopping.len() {
                let (_, _, pod, since) = &mut st.stopping[i];
                let since = *since;
                if pod.stopped(since) {
                    let (id, k, _, _) = st.stopping.remove(i);
                    restart.push((id, k));
                } else {
                    i += 1;
                }
            }
            st.retiring.retain_mut(|(p, since)| !p.stopped(*since));
        }
        for (id, k) in restart {
            let demo = self.state.lock().unwrap().sets.get(&id).map(|(d, _)| *d);
            let Some(demo) = demo else { continue };
            let pod = self.start(id, k, demo);
            if let Some((_, pods)) = self.state.lock().unwrap().sets.get_mut(&id) {
                pods.insert(k, pod);
            }
        }
        let want = self.state.lock().unwrap().deployment.want.clone();
        if let Some((list, replicas, spec)) = want {
            let config = spec.coordinator_config();
            let mut st = self.state.lock().unwrap();
            let d = &mut st.deployment;
            let fresh = |p: &CoordPod| p.pod.config == config && p.list == list;
            let n_fresh = d.pods.iter().filter(|p| fresh(p)).count();
            let up = |p: &CoordPod| ready.contains(&p.pod.http);
            let n_up = d.pods.iter().filter(|p| up(p)).count();
            let mut start = false;
            let mut stop = None;
            if n_fresh < replicas && d.pods.len() < replicas + 1 {
                start = true;
            } else if let Some(i) = d.pods.iter().position(|p| !fresh(p) && !up(p)) {
                // An old one that does not serve goes at once.
                stop = Some(i);
            } else if let Some(i) = d.pods.iter().position(|p| !fresh(p)) {
                if n_up > replicas || (n_up == replicas && d.pods.iter().filter(|p| fresh(p) && up(p)).count() >= replicas) {
                    stop = Some(i);
                }
            } else if n_fresh > replicas {
                stop = d.pods.iter().position(|p| !up(p)).or(Some(0));
            }
            if let Some(i) = stop {
                let p = d.pods.remove(i);
                p.pod.terminate();
                st.retiring.push((p.pod, Instant::now()));
            }
            drop(st);
            if start {
                let p = self.start_coordinator(&list, &spec);
                self.state.lock().unwrap().deployment.pods.push(p);
            }
        }
        if let Some(svc) = self.service.get() {
            let st = self.state.lock().unwrap();
            *svc.ports.lock().unwrap() = st.deployment.pods.iter().map(|p| p.pod.http).chain(st.retiring.iter().map(|(p, _)| p.http)).collect();
        }
    }

    /// Copy `k` of set `id` comes back without its volume.
    fn lose_volume(&self, id: u32, k: usize) {
        let demo = {
            let mut st = self.state.lock().unwrap();
            let (demo, pods) = st.sets.get_mut(&id).unwrap();
            drop(pods.remove(&k));
            *demo
        };
        std::fs::remove_dir_all(self.volume(id, k)).unwrap();
        let pod = self.start(id, k, demo);
        self.state.lock().unwrap().sets.get_mut(&id).unwrap().1.insert(k, pod);
    }

    /// The process ids of the copies, by set and ordinal.
    fn pids(&self) -> BTreeMap<(u32, usize), u32> {
        let st = self.state.lock().unwrap();
        st.sets.iter().flat_map(|(&id, (_, pods))| pods.iter().filter_map(move |(&k, p)| Some(((id, k), p.child.as_ref()?.id())))).collect()
    }

    fn logs(&self) -> String {
        let mut out = String::new();
        for e in std::fs::read_dir(&self.dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "log") {
                let text = std::fs::read_to_string(e.path()).unwrap_or_default();
                let tail: Vec<&str> = text.lines().rev().take(15).collect();
                out += &format!("--- {}\n{}\n", e.path().display(), tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
            }
        }
        out
    }
}

/// One client for every call the tests make: building one can take a
/// second on macOS (`strata_server::http_client`), and a step asks every
/// server whether it is ready. It keeps no idle connections, so none
/// outlives the test (and runtime) that opened it.
fn http() -> &'static reqwest::Client {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    HTTP.get_or_init(|| strata_server::http_client().pool_max_idle_per_host(0).build().unwrap())
}

async fn ready(http: u16) -> bool {
    self::http().get(format!("http://127.0.0.1:{http}/readyz")).send().await.is_ok_and(|r| r.status().is_success())
}

impl Platform for Procs {
    async fn sets(&self) -> Result<Vec<Set>, String> {
        self.drive();
        let pods: Vec<(u32, usize, Vec<(usize, u16)>)> = {
            let st = self.state.lock().unwrap();
            let stopping = |id: u32| st.stopping.iter().filter(|(s, _, _, _)| *s == id).count();
            st.sets.iter().map(|(&id, (_, pods))| (id, pods.len() + stopping(id), pods.iter().map(|(&k, p)| (k, p.http)).collect())).collect()
        };
        let mut sets = Vec::new();
        for (id, replicas, pods) in pods {
            let mut up = Vec::new();
            for (k, http) in pods {
                if ready(http).await {
                    up.push(k);
                }
            }
            sets.push(Set { id, replicas, ready: up });
        }
        Ok(sets)
    }

    async fn create_set(&self, id: u32, replicas: usize, demo: Option<(usize, usize)>) -> Result<(), String> {
        let spec = self.spec.lock().unwrap().clone();
        self.state.lock().unwrap().configs.insert(id, spec);
        let pods = (0..replicas).map(|k| (k, self.start(id, k, demo))).collect();
        self.state.lock().unwrap().sets.insert(id, (demo, pods));
        Ok(())
    }

    async fn scale_set(&self, id: u32, replicas: usize) -> Result<(), String> {
        let (demo, have) = {
            let st = self.state.lock().unwrap();
            let (demo, pods) = &st.sets[&id];
            (*demo, pods.len())
        };
        for k in have..replicas {
            let pod = self.start(id, k, demo);
            self.state.lock().unwrap().sets.get_mut(&id).unwrap().1.insert(k, pod);
        }
        for k in replicas..have {
            drop(self.state.lock().unwrap().sets.get_mut(&id).unwrap().1.remove(&k));
            // The pod's volume goes with it (whenScaled: Delete).
            let _ = std::fs::remove_dir_all(self.volume(id, k));
        }
        Ok(())
    }
    async fn delete_set(&self, id: u32) -> Result<(), String> {
        let pods = {
            let mut st = self.state.lock().unwrap();
            st.configs.remove(&id);
            st.sets.remove(&id)
        };
        if let Some((_, pods)) = pods {
            // Stopped first: a server still running writes into the
            // volume as it goes, and the volume is left behind.
            let copies: Vec<usize> = pods.keys().copied().collect();
            drop(pods);
            for k in copies {
                let _ = std::fs::remove_dir_all(self.volume(id, k));
            }
        }
        Ok(())
    }

    async fn coordinator(&self) -> Result<Option<(Vec<Vec<String>>, usize)>, String> {
        self.drive();
        Ok(self.state.lock().unwrap().deployment.want.as_ref().map(|(l, n, _)| (l.clone(), *n)))
    }

    async fn set_coordinator(&self, shards: &[Vec<String>], replicas: usize) -> Result<(), String> {
        self.service.get_or_init(|| Service::start(self.coordinator_port)).await;
        let spec = self.spec.lock().unwrap().clone();
        self.state.lock().unwrap().deployment.want = Some((shards.to_vec(), replicas, spec));
        self.drive();
        Ok(())
    }

    async fn coordinator_config(&self) -> Result<Option<String>, String> {
        Ok(self.state.lock().unwrap().deployment.want.as_ref().map(|(_, _, s)| s.coordinator_config()))
    }

    async fn coordinators_rolled(&self) -> Result<Rolled, String> {
        self.drive();
        let ready: Vec<u16> = self.service.get().map(|s| s.ready.lock().unwrap().clone()).unwrap_or_default();
        let st = self.state.lock().unwrap();
        let Some((list, n, spec)) = &st.deployment.want else { return Ok(Rolled { done: true, text: "no coordinators".into() }) };
        let fresh: Vec<&CoordPod> = st.deployment.pods.iter().filter(|p| p.pod.config == spec.coordinator_config() && p.list == *list).collect();
        let up = fresh.iter().filter(|p| ready.contains(&p.pod.http)).count();
        let old = st.deployment.pods.len() - fresh.len();
        let done = fresh.len() == *n && up == *n && old == 0;
        Ok(Rolled { done, text: format!("{} of {n} restarted, {up} ready{}", fresh.len(), if old > 0 { format!(", {old} older still serving") } else { String::new() }) })
    }

    async fn trouble(&self, spec: &Spec) -> Result<Vec<Trouble>, String> {
        let (server, coordinator) = (spec.server_config(), spec.coordinator_config());
        let mut st = self.state.lock().unwrap();
        let mut out = Vec::new();
        for (&id, (_, pods)) in st.sets.iter_mut() {
            for (&k, pod) in pods.iter_mut() {
                if let Some(text) = pod.trouble() {
                    out.push(Trouble { copy: Some((id, k)), who: format!("copy {id}-{k}"), fresh: pod.config == server, text });
                }
            }
        }
        for p in st.deployment.pods.iter_mut() {
            if let Some(text) = p.pod.trouble() {
                out.push(Trouble { copy: None, who: format!("coordinator :{}", p.pod.http), fresh: p.pod.config == coordinator, text });
            }
        }
        Ok(out)
    }

    async fn preflight(&self, image: &str) -> Result<Check, String> {
        {
            let st = self.state.lock().unwrap();
            let all = st.sets.values().flat_map(|(_, pods)| pods.values()).chain(st.deployment.pods.iter().map(|p| &p.pod)).all(|p| p.image == image);
            if all {
                return Ok(Check::Done("every server runs it already".into()));
            }
        }
        self.preflights.lock().unwrap().push(image.to_string());
        let Some((bin, _)) = binary(image) else { return Ok(Check::Failed(format!("ErrImagePull: the image {image} cannot be pulled"))) };
        let out = Command::new(bin).arg("--help").output().map_err(|e| e.to_string())?;
        Ok(match out.status.success() {
            true => Check::Done("it runs on this machine".into()),
            false => Check::Failed(format!("strata-server --help exited with {}", out.status)),
        })
    }

    async fn end_preflight(&self) -> Result<(), String> {
        Ok(())
    }

    async fn configure(&self, id: u32, spec: &Spec) -> Result<bool, String> {
        let mut st = self.state.lock().unwrap();
        let changed = st.configs.get(&id).is_none_or(|c| c.server_config() != spec.server_config());
        if changed {
            st.configs.insert(id, spec.clone());
        }
        Ok(changed)
    }

    async fn stale(&self, spec: &Spec) -> Result<Vec<(u32, usize)>, String> {
        let want = spec.server_config();
        let st = self.state.lock().unwrap();
        Ok(st.sets.iter().flat_map(|(&id, (_, pods))| pods.iter().filter(|(_, p)| p.config != want).map(move |(&k, _)| (id, k))).collect())
    }

    async fn restart(&self, id: u32, ordinal: usize) -> Result<(), String> {
        // As the kubelet stops a pod: SIGTERM, and a kill after the grace
        // period; the StatefulSet makes it again once it stopped.
        let mut st = self.state.lock().unwrap();
        let (_, pods) = st.sets.get_mut(&id).ok_or("no such set")?;
        let pod = pods.remove(&ordinal).ok_or("no such copy")?;
        pod.terminate();
        st.stopping.push((id, ordinal, pod, Instant::now()));
        self.restarts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn grow_volumes(&self, _size: &str, _apply: bool) -> Result<usize, String> {
        // A directory has no size.
        Ok(0)
    }

    fn url(&self, id: u32, k: usize) -> String {
        format!("http://127.0.0.1:{}", self.ports(id, k).1)
    }

    fn set_of(&self, url: &str) -> Option<(u32, usize)> {
        let p: u16 = url.rsplit(':').next()?.parse().ok()?;
        self.state.lock().unwrap().ports.iter().find(|(_, &(_, g))| g == p).map(|(&k, _)| k)
    }
}

/// Rollouts quick enough for a test: verified for 2 s, a check failing
/// for 2 s fails the version, a step may take a minute.
fn quick() -> RolloutSpec {
    RolloutSpec { verify_seconds: 2, hold_after_seconds: 2, progress_deadline_seconds: 60, ..RolloutSpec::default() }
}

struct Run {
    procs: Procs,
    admin: Admin,
    /// The spec as the test last changed it.
    spec: Spec,
    /// What the last step left for the next (the resource's
    /// `status.rollout`).
    rollout: Mutex<Rollout>,
    /// The cluster's versions (`status.history`).
    history: Mutex<Json>,
    /// The spec the operator rolled a failed version back to, while the
    /// test's spec is that version (its hash, and the spec), as the
    /// operator patches a `StrataCluster`'s spec.
    back: Mutex<Option<(String, Spec)>>,
}

impl Run {
    fn new(procs: Procs, admin: Admin, spec: Spec) -> Run {
        Run { procs, admin, spec, rollout: Mutex::new(Rollout::default()), history: Mutex::new(Json::Null), back: Mutex::new(None) }
    }

    /// The spec as the operator works to it: the test's, or the version
    /// it rolled back to.
    fn current(&self) -> Spec {
        match &*self.back.lock().unwrap() {
            Some((v, s)) if *v == self.spec.version() => s.clone(),
            _ => self.spec.clone(),
        }
    }

    /// One step toward the spec, and what the operator does with a
    /// version that failed (as `kube::once` does with a `StrataCluster`):
    /// back to the last version that ran everywhere.
    async fn once(&self) -> Outcome {
        let spec = self.current();
        *self.procs.spec.lock().unwrap() = spec.clone();
        let rollback = (spec != self.spec).then(|| spec.version());
        let before = noted(self.rollout.lock().unwrap().clone(), None, rollback.as_deref(), &spec);
        let mut out = reconcile(&spec, &self.procs, &self.admin, &before).await;
        let mut h = self.history.lock().unwrap();
        *h = versions(&h, &spec, None, None, strata_server::operator::now_ms(), out.phase == Phase::Ready);
        let mut next = out.rollout.clone();
        if let Some(f) = out.rollout.failed.clone().filter(|_| spec.rollout.on_failure == OnFailure::Rollback) {
            if let Some(t) = rollback_target(&h, &spec, out.rollout.ungated) {
                let now = serde_json::to_value(&spec).unwrap();
                let (_, back) = patched(&now, &back_to(&now, &t["spec"])).unwrap();
                out.message = format!("rolling back to version {}: the version failed ({}): {}", t["version"], f.step, f.text);
                next = Rollout::default();
                *self.back.lock().unwrap() = Some((self.spec.version(), back));
            }
        }
        *self.rollout.lock().unwrap() = next;
        out
    }

    /// Steps until `done` holds of a step, with every step's message.
    async fn until(&self, what: &str, done: impl Fn(&Outcome) -> bool) -> (Outcome, Vec<String>) {
        let t0 = Instant::now();
        let mut said: Vec<String> = Vec::new();
        loop {
            let out = self.once().await;
            if said.last() != Some(&out.message) {
                said.push(out.message.clone());
            }
            if done(&out) {
                return (out, said);
            }
            if out.phase == Phase::Failed {
                panic!("{what}: failed: {}\n{}", out.message, said.join("\n"));
            }
            if t0.elapsed() > Duration::from_secs(120) {
                panic!("{what}: not there after 120 s:\n{}\n{}", said.join("\n"), self.procs.logs());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Steps until the cluster is as the spec says, with every step's
    /// message.
    async fn settle(&self, what: &str) -> Vec<String> {
        self.until(what, |o| o.phase == Phase::Ready).await.1
    }

    async fn sql(&self, sql: &str) -> Json {
        let a = self.admin.call(reqwest::Method::POST, "/v1/sql", Some(json!({"sql": sql}))).await;
        a.unwrap_or_else(|e| panic!("{sql}: {e}"))
    }

    async fn count(&self) -> i64 {
        self.sql("SELECT COUNT(*) FROM products").await["rows"][0][0].as_i64().unwrap()
    }

    async fn sold(&self) -> i64 {
        self.sql("SELECT SUM(sold) FROM products WHERE sold >= 100000").await["rows"][0][0].as_i64().unwrap_or(0)
    }

    async fn cluster(&self) -> Json {
        self.admin.call(reqwest::Method::GET, "/v1/cluster", None).await.unwrap()
    }

    /// Every server started with `--slow-ms` `ms`.
    async fn slow_ms_everywhere(&self, ms: f64) {
        let servers: Vec<u16> = self.procs.state.lock().unwrap().sets.values().flat_map(|(_, pods)| pods.values().map(|p| p.http)).collect();
        assert_eq!(servers.len(), 9);
        for http in servers {
            let mut r = self::http().get(format!("http://127.0.0.1:{http}/v1/profile/settings"));
            if let Some((_, cluster)) = &self.procs.tokens {
                r = r.bearer_auth(cluster);
            }
            let j: Json = r.send().await.unwrap().json().await.unwrap();
            assert_eq!(j["started_slow_ms"], json!(ms), "{http}: {j}");
        }
    }

    /// Servers per shard and slots per shard.
    async fn shape(&self) -> (Vec<usize>, Vec<u64>) {
        let c = self.cluster().await;
        let shards = c["shards"].as_array().unwrap();
        (shards.iter().map(|s| s["replicas"].as_array().unwrap().len()).collect(), shards.iter().map(|s| s["slots_held"].as_u64().unwrap()).collect())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_is_made_grown_shrunk_and_healed() {
    let dir = tempfile::tempdir().unwrap();
    let procs = Procs::new(dir.path(), None);
    let admin = Admin::new(&format!("http://127.0.0.1:{}", procs.coordinator_port), None);
    let spec = Spec { shards: 2, replicas: 2, demo: Some(ROWS), failover_after_ms: 1000, rollout: quick(), ..Spec::default() };
    every_step(&mut Run::new(procs, admin, spec)).await;
}

/// The same steps with a cluster token: the operator holds the admin
/// token, which the servers do not take, and every step the coordinator
/// passes on to them goes with the cluster's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_with_its_own_token_is_made_grown_shrunk_and_healed() {
    let dir = tempfile::tempdir().unwrap();
    let procs = Procs::new(dir.path(), Some(("admin-token", "cluster-token")));
    let admin = Admin::new(&format!("http://127.0.0.1:{}", procs.coordinator_port), Some("admin-token".into()));
    let spec = Spec {
        shards: 2,
        replicas: 2,
        demo: Some(ROWS),
        failover_after_ms: 1000,
        token_secret: Some("admin".into()),
        cluster_token_secret: Some("inner".into()),
        rollout: quick(),
        ..Spec::default()
    };
    let mut run = Run::new(procs, admin, spec);
    every_step(&mut run).await;
    // A server takes the cluster's token, not the admin's.
    let server = run.procs.state.lock().unwrap().sets[&0].1[&0].http;
    let ask = |token: &'static str| http().get(format!("http://127.0.0.1:{server}/v1/stats")).bearer_auth(token).send();
    assert_eq!(ask("admin-token").await.unwrap().status(), 401);
    assert_eq!(ask("cluster-token").await.unwrap().status(), 200);
}

/// A copy that cannot start with its settings (here an argument its
/// server does not take; on Kubernetes, requests no node has room for)
/// restarts at once with the settings that fix it, rather than being
/// waited for, while the cluster is made. A rollout of a bad setting
/// fails on the third copy a shard of two takes first, and is rolled
/// back without any copy of the shard's own restarting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_copy_that_cannot_start_restarts_with_the_settings_that_fix_it() {
    let dir = tempfile::tempdir().unwrap();
    let procs = Procs::new(dir.path(), None);
    let admin = Admin::new(&format!("http://127.0.0.1:{}", procs.coordinator_port), None);
    let bad = vec!["--no-such-flag".to_string()];
    let spec = Spec { shards: 1, replicas: 2, demo: Some(ROWS), failover_after_ms: 1000, args: bad.clone(), rollout: quick(), ..Spec::default() };
    let mut run = Run::new(procs, admin, spec);

    // Made with an argument no copy starts with: it waits for them.
    for _ in 0..20 {
        let o = run.once().await;
        assert!(o.phase == Phase::Creating && !o.message.contains("which has not started"), "{o:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Fixed, the copies restart with it at once.
    run.spec.args = Vec::new();
    let said = run.settle("made once its settings are fixed").await;
    assert!(said.iter().any(|m| m.contains("which has not started")), "{said:?}");
    assert_eq!(run.shape().await.0, vec![2]);
    assert_eq!(run.count().await, ROWS as i64);

    // The bad argument rolls out to the third copy the shard of two takes
    // first, which exits: the version fails before either of the shard's
    // own copies restarts, and goes back to the one that ran everywhere;
    // the third copy goes.
    let pids = run.procs.pids();
    run.spec.args = bad;
    let (_, said) = run.until("rolled back", |o| o.phase == Phase::Ready && run.current().args.is_empty()).await;
    assert!(said.iter().any(|m| m.contains("starts as a third copy with the new settings")), "{said:?}");
    let failed = said.iter().find(|m| m.starts_with("rolling back to version 2: the version failed (copies):")).unwrap_or_else(|| panic!("{said:?}"));
    assert!(failed.contains("copy 0-2 cannot run the new settings") && failed.contains("exit status: 2"), "{failed}");
    assert!(!said.iter().any(|m| m.starts_with("restarting")), "{said:?}");
    assert_eq!(run.procs.pids(), pids, "none of the shard's own copies restarted");
    assert_eq!(run.shape().await.0, vec![2]);
    assert_eq!(run.count().await, ROWS as i64);
    // A failed rollback is not rolled back again: held instead.
    let h = run.history.lock().unwrap().clone();
    assert_eq!(h.as_array().unwrap().iter().map(|v| v["version"].as_u64().unwrap()).collect::<Vec<_>>(), vec![1, 2, 3, 4], "{h}");
    assert!(rollback_target(&h, &run.current(), true).is_none());
}

/// Makes the cluster, grows it in shards and copies, heals a copy that
/// lost its volume, hands a primary over, shrinks it back and starts the
/// coordinator again, every row and write kept.
async fn every_step(run: &mut Run) {

    // Made: two shards of two copies, the demo products split over them.
    let said = run.settle("made").await;
    assert!(said.iter().any(|m| m.contains("starting the coordinator")), "{said:?}");
    assert_eq!(run.shape().await, (vec![2, 2], vec![2048, 2048]));
    assert_eq!(run.count().await, ROWS as i64);
    // Writes that each step must keep.
    let mut sum = 0;
    for i in 0..30 {
        let v = 100_000 + i;
        run.sql(&format!("UPDATE products SET sold = {v} WHERE id = 'p{i:07}'")).await;
        sum += v;
    }
    assert_eq!(run.sold().await, sum);

    // A third shard, which takes a third of the slots. Paused while they
    // move, the moves stop after the one under way, and go on once the
    // pause is lifted.
    run.spec.shards = 3;
    let (_, mut said) = run.until("moving", |o| o.message.contains("evening out the slots")).await;
    run.spec.rollout.paused = true;
    let (out, more) = run.until("paused", |o| o.phase == Phase::Paused).await;
    said.extend(more);
    assert!(out.message.starts_with("paused (spec.rollout.paused); next:"), "{out:?}");
    let t0 = Instant::now();
    while run.admin.call(reqwest::Method::GET, "/v1/admin/moves", None).await.unwrap()["job"]["state"] == "running" {
        assert!(t0.elapsed() < Duration::from_secs(30), "the slot moves did not stop: {said:?}");
        assert_eq!(run.once().await.phase, Phase::Paused);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for _ in 0..5 {
        let o = run.once().await;
        assert!(o.phase == Phase::Paused && o.message.contains("slots"), "{o:?}");
    }
    run.spec.rollout.paused = false;
    said.extend(run.settle("three shards").await);
    assert!(said.iter().any(|m| m.contains("joins the cluster")) && said.iter().any(|m| m.contains("evening out the slots")), "{said:?}");
    let (servers, slots) = run.shape().await;
    assert_eq!(servers, vec![2, 2, 2]);
    assert!(slots.iter().all(|&s| (1365..=1366).contains(&s)), "{slots:?}");
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));

    // Three copies each.
    run.spec.replicas = 3;
    run.settle("three copies").await;
    assert_eq!(run.shape().await.0, vec![3, 3, 3]);

    // A copy that comes back without its volume leaves and joins again,
    // and is sent a copy of its primary.
    run.procs.lose_volume(1, 1);
    let lost = run.procs.url(1, 1);
    let t0 = Instant::now();
    loop {
        let said = run.settle("healed").await;
        if said.iter().any(|m| m.contains("came back without its data")) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "the copy that lost its volume was not found: {said:?}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let c = run.cluster().await;
    assert!(c["shards"].as_array().unwrap().iter().any(|s| s["replicas"].as_array().unwrap().iter().any(|r| r["url"] == json!(lost))), "{c}");
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));

    // New settings: every copy starts again with them, one at a time, each
    // primary handing its shard over to a copy in step before it goes.
    run.spec.args = vec!["--slow-ms".into(), "777".into()];
    let said = run.settle("new settings").await;
    assert!(said.iter().any(|m| m.contains("restarting") && m.contains("with the new settings")), "{said:?}");
    assert!(said.iter().any(|m| m.contains("before") && m.contains("restarts with the new settings")), "{said:?}");
    run.slow_ms_everywhere(777.0).await;
    assert!(run.procs.stale(&run.spec).await.unwrap().is_empty());
    assert_eq!(run.shape().await.0, vec![3, 3, 3]);
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));

    // Paused, the next settings wait.
    run.spec.args = vec!["--slow-ms".into(), "778".into()];
    run.spec.rollout.paused = true;
    let (out, _) = run.until("paused", |o| o.phase == Phase::Paused).await;
    assert!(out.message.starts_with("paused") && out.message.contains("restarting") && out.message.contains("(9 copies left)"), "{out:?}");
    assert!(out.rollout.paused && out.rollout.stale.len() == 9 && out.rollout.copies == 9, "{:?}", out.rollout);
    for _ in 0..10 {
        assert_eq!(run.once().await.phase, Phase::Paused);
    }
    assert_eq!(run.procs.stale(&run.spec).await.unwrap().len(), 9, "no copy restarts while paused");
    // Gated: the fullest volume over 0.01% of its disk, which every volume
    // here is, for a second holds the rollout, until it is resumed.
    run.spec.rollout = RolloutSpec { paused: false, gates: Gates { disk_used_ratio: Some(0.0001), ..Gates::default() }, hold_after_seconds: 1, soak_seconds: 1, on_failure: OnFailure::Hold, ..quick() };
    let (held, _) = run.until("held", |o| o.rollout.held.is_some()).await;
    let h = held.rollout.held.clone().unwrap();
    assert!(h.gate == "diskUsedRatio" && h.text.starts_with("the fullest volume was") && h.text.contains("over its 0.01% limit"), "{h:?}");
    assert!(held.phase == Phase::Paused && held.message.starts_with("held: the fullest volume was"), "{held:?}");
    for _ in 0..10 {
        let o = run.once().await;
        assert_eq!((o.phase, o.rollout.held.as_ref()), (Phase::Paused, Some(&h)), "{o:?}");
    }
    let left = run.procs.stale(&run.spec).await.unwrap().len();
    assert!((7..=9).contains(&left), "{left} copies left: a second goes before the gate holds");
    // A resume starts the gate's clock again: a second later it holds again.
    let resumed = noted(run.rollout.lock().unwrap().clone(), Some(&h.at_ms.to_string()), None, &run.spec);
    assert!(resumed.held.is_none() && resumed.gates.iter().all(|g| g.over_since_ms.is_none()), "{resumed:?}");
    *run.rollout.lock().unwrap() = resumed;
    let (again, _) = run.until("held again", |o| o.rollout.held.is_some()).await;
    assert!(again.rollout.held.as_ref().unwrap().at_ms > h.at_ms, "{again:?}");
    // Its limit raised over the number, the hold lifts, and the rest
    // restart, each restarted copy serving a second before the next.
    run.spec.rollout.gates.disk_used_ratio = Some(1.0);
    let said = run.settle("gate raised").await;
    assert!(said.iter().any(|m| m.contains("serves 1 s with them before the next restarts")), "{said:?}");
    run.slow_ms_everywhere(778.0).await;
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));
    run.spec.rollout = quick();

    // Two copies each again. The highest copy of the first set is made its
    // shard's primary first: it hands the primary over before it goes.
    let top = run.procs.url(0, 2);
    let shard = c["shards"].as_array().unwrap().iter().position(|s| s["replicas"].as_array().unwrap().iter().any(|r| r["url"] == json!(top))).unwrap();
    run.admin.call(reqwest::Method::POST, &format!("/v1/admin/shards/{shard}/promote"), Some(json!({"url": top}))).await.unwrap();
    run.spec.replicas = 2;
    let said = run.settle("two copies").await;
    assert!(said.iter().any(|m| m.contains("handing the primary over")), "{said:?}");
    assert_eq!(run.shape().await.0, vec![2, 2, 2]);
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));

    // Two shards again: the newest is drained, removed and deleted.
    run.spec.shards = 2;
    let said = run.settle("two shards").await;
    assert!(said.iter().any(|m| m.contains("draining shard")) && said.iter().any(|m| m.contains("leaves the cluster")), "{said:?}");
    assert_eq!(run.shape().await, (vec![2, 2], vec![2048, 2048]));
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));
    assert!(!run.procs.volume(2, 0).exists(), "the removed shard's volumes are deleted");
    // And the coordinator, killed and started again on its list, serves
    // it all.
    run.procs.state.lock().unwrap().deployment.pods.clear();
    run.procs.drive();
    let t0 = Instant::now();
    while run.admin.call(reqwest::Method::GET, "/v1/cluster", None).await.is_err() {
        assert!(t0.elapsed() < Duration::from_secs(30), "the coordinator did not start again: {}", run.procs.logs());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    run.settle("coordinator again").await;
    assert_eq!((run.count().await, run.sold().await), (ROWS as i64, sum));
}

/// What the clients saw while the cluster changed: reads (searches, key
/// lookups, counts) and writes through the coordinators' Service, each
/// with the answers that failed.
#[derive(Debug, Default)]
struct Seen {
    reads: usize,
    writes: usize,
    failed: Vec<String>,
    slowest_ms: f64,
    /// The last value each key was written, as acknowledged.
    written: BTreeMap<String, i64>,
    /// Writes answered in doubt: applied by a primary that another
    /// replaced before its copies acknowledged them (409), which the
    /// coordinator does not send again. Each key's values written so since
    /// its last acknowledged one.
    in_doubt: Vec<String>,
    doubted: BTreeMap<String, Vec<i64>>,
    /// Increments of [`COUNTER`] acknowledged, and in doubt: a write
    /// applied twice (or an acknowledged one lost) shows in its value.
    added: i64,
    added_in_doubt: i64,
}

/// The key the load adds 1 to again and again (`sold = sold + 1`), a
/// write that is wrong when applied twice.
const COUNTER: &str = "p0000299";

async fn counter(run: &Run) -> i64 {
    run.sql(&format!("SELECT sold FROM products WHERE id = '{COUNTER}'")).await["rows"][0][0].as_i64().unwrap()
}

/// Clients sending queries and writes all along, as an application would.
struct Load {
    stop: Arc<AtomicBool>,
    seen: Arc<Mutex<Seen>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Load {
    fn start(port: u16, readers: usize) -> Load {
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Seen::default()));
        let url = format!("http://127.0.0.1:{port}/v1/sql");
        let url3 = url.clone();
        let mut tasks = Vec::new();
        let ask = |http: reqwest::Client, url: String, sql: String| async move {
            let t0 = Instant::now();
            let r = http.post(&url).json(&json!({"sql": sql})).send().await;
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let r = match r {
                Ok(a) if a.status().is_success() => Ok(a.json::<Json>().await.unwrap_or(Json::Null)),
                Ok(a) => Err(format!("{}: {}", a.status(), a.text().await.unwrap_or_default())),
                Err(e) => Err(format!("{e:?}")),
            };
            (r, ms)
        };
        for n in 0..readers {
            let (stop, seen, url) = (stop.clone(), seen.clone(), url.clone());
            tasks.push(tokio::spawn(async move {
                let http = strata_server::http_client().timeout(Duration::from_secs(15)).build().unwrap();
                let mut i = n;
                while !stop.load(Ordering::Relaxed) {
                    i += 1;
                    let sql = match i % 3 {
                        0 => "SELECT id, title FROM products ORDER BY bm25(body, 'laptop phone tent') DESC LIMIT 5".to_string(),
                        1 => format!("SELECT id, sold FROM products WHERE id = 'p{:07}'", i % ROWS),
                        _ => "SELECT COUNT(*) FROM products".to_string(),
                    };
                    let (r, ms) = ask(http.clone(), url.clone(), sql.clone()).await;
                    let mut s = seen.lock().unwrap();
                    s.reads += 1;
                    s.slowest_ms = s.slowest_ms.max(ms);
                    match r {
                        Ok(j) if i % 3 == 2 && j["rows"][0][0] != json!(ROWS) => s.failed.push(format!("{sql}: {j}")),
                        Ok(_) => {}
                        Err(e) => s.failed.push(format!("{sql}: {e}")),
                    }
                }
            }));
        }
        let (stop2, seen2) = (stop.clone(), seen.clone());
        tasks.push(tokio::spawn(async move {
            let http = strata_server::http_client().timeout(Duration::from_secs(15)).build().unwrap();
            let mut i = 0i64;
            while !stop2.load(Ordering::Relaxed) {
                i += 1;
                let key = format!("p{:07}", i as usize % 40);
                let sql = format!("UPDATE products SET sold = {} WHERE id = '{key}'", 500_000 + i);
                let (r, ms) = ask(http.clone(), url.clone(), sql.clone()).await;
                {
                    let mut s = seen2.lock().unwrap();
                    s.writes += 1;
                    s.slowest_ms = s.slowest_ms.max(ms);
                    match r {
                        Ok(_) => {
                            s.doubted.remove(&key);
                            s.written.insert(key, 500_000 + i);
                        }
                        Err(e) if e.contains(IN_DOUBT) => {
                            s.doubted.entry(key).or_default().push(500_000 + i);
                            s.in_doubt.push(format!("{sql}: {e}"));
                        }
                        Err(e) => s.failed.push(format!("{sql}: {e}")),
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }));
        let (stop3, seen3) = (stop.clone(), seen.clone());
        tasks.push(tokio::spawn(async move {
            let http = strata_server::http_client().timeout(Duration::from_secs(15)).build().unwrap();
            while !stop3.load(Ordering::Relaxed) {
                let sql = format!("UPDATE products SET sold = sold + 1 WHERE id = '{COUNTER}'");
                let (r, ms) = ask(http.clone(), url3.clone(), sql.clone()).await;
                {
                    let mut s = seen3.lock().unwrap();
                    s.writes += 1;
                    s.slowest_ms = s.slowest_ms.max(ms);
                    match r {
                        Ok(_) => s.added += 1,
                        Err(e) if e.contains(IN_DOUBT) => {
                            s.added_in_doubt += 1;
                            s.in_doubt.push(format!("{sql}: {e}"));
                        }
                        Err(e) => s.failed.push(format!("{sql}: {e}")),
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }));
        Load { stop, seen, tasks }
    }

    async fn end(self) -> Seen {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.tasks {
            let _ = t.await;
        }
        Arc::try_unwrap(self.seen).unwrap().into_inner().unwrap()
    }

    fn so_far(&self) -> (usize, usize, Vec<String>) {
        let s = self.seen.lock().unwrap();
        (s.reads, s.writes, s.failed.clone())
    }
}

/// A new version rolls out while queries and writes come in: the image is
/// tried first, the coordinators restart (a new one ready before an old
/// one stops), each shard of two copies takes a third with the new
/// version, then each copy restarts, its primary handed over first, the
/// third goes, and the version is verified. No query or write fails, and
/// every write is kept.
/// An image whose servers exit fails at its first copy and is rolled back
/// to the version before, still without a failed query; one that cannot
/// be pulled is refused before any server restarts.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_new_version_rolls_out_under_load_and_a_bad_one_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let procs = Procs::new(dir.path(), None);
    let port = procs.coordinator_port;
    let admin = Admin::new(&format!("http://127.0.0.1:{port}"), None);
    let spec = Spec { image: "strata-server:v1".into(), shards: 2, replicas: 2, coordinators: 2, demo: Some(ROWS), failover_after_ms: 1000, rollout: quick(), ..Spec::default() };
    let mut run = Run::new(procs, admin, spec);
    run.settle("made").await;
    assert_eq!(run.shape().await.0, vec![2, 2]);
    let counted = counter(&run).await;
    let load = Load::start(port, 4);
    tokio::time::sleep(Duration::from_secs(2)).await;

    // v2: tried, then every coordinator and copy restarts, one at a time.
    let t0 = Instant::now();
    run.spec.image = "strata-server:v2".into();
    let said = run.settle("v2").await;
    let took = t0.elapsed();
    for step in [
        "restarting the coordinators with the new settings",
        "starts as a third copy with the new settings, so that most of its copies serve while each of its 2 restarts",
        "handing the primary over",
        "restarting http://",
        "leaves shard",
        "verifying the version: healthy for 2 s in a row (4 copies back and in sync, a query through the coordinators answered)",
    ] {
        assert!(said.iter().any(|m| m.contains(step)), "{step}: {said:#?}");
    }
    // One handover a shard: each primary goes to its own restarted copy,
    // and the third leaves as a replica.
    assert_eq!(said.iter().filter(|m| m.contains("handing the primary over")).count(), 2, "{said:#?}");
    assert!(run.procs.preflights.lock().unwrap().contains(&"strata-server:v2".to_string()));
    assert_eq!(run.procs.restarts.load(Ordering::Relaxed), 4);
    assert_eq!(run.shape().await.0, vec![2, 2]);
    assert!(run.procs.stale(&run.spec).await.unwrap().is_empty());
    let (reads, writes, failed) = load.so_far();
    assert!(failed.is_empty(), "{} of {reads} reads and {writes} writes failed during the rollout:\n{}", failed.len(), failed.join("\n"));
    println!("v2 rolled out in {:.1} s: {reads} reads, {writes} writes, none failed", took.as_secs_f64());

    // v3's servers exit: its first copy fails, and v2 comes back.
    run.spec.image = "strata-server:crashes".into();
    let (_, said) = run.until("v3 rolled back", |o| o.phase == Phase::Ready && run.current().image.ends_with(":v2")).await;
    let back = said.iter().find(|m| m.starts_with("rolling back to version 2:")).unwrap_or_else(|| panic!("{said:#?}"));
    assert!(back.contains("cannot run the new settings") && back.contains("exit status: 2"), "{back}");
    assert!(run.procs.stale(&run.current()).await.unwrap().is_empty());
    assert_eq!(run.shape().await.0, vec![2, 2]);
    let (reads, writes, failed) = load.so_far();
    assert!(failed.is_empty(), "{} of {reads} reads and {writes} writes failed:\n{}", failed.len(), failed.join("\n"));

    // v4 cannot be pulled: refused before any server restarts.
    let pids = run.procs.pids();
    let restarts = run.procs.restarts.load(Ordering::Relaxed);
    run.spec.image = "strata-server:missing".into();
    let (_, said) = run.until("v4 refused", |o| o.phase == Phase::Ready && run.current().image.ends_with(":v2")).await;
    let back = said.iter().find(|m| m.contains("the version failed (preflight)")).unwrap_or_else(|| panic!("{said:#?}"));
    assert!(back.contains("no server was restarted with it") && back.contains("cannot be pulled"), "{back}");
    assert_eq!((run.procs.pids(), run.procs.restarts.load(Ordering::Relaxed)), (pids, restarts), "no copy restarted");

    let seen = load.end().await;
    assert!(seen.failed.is_empty(), "{} of {} reads and {} writes failed:\n{}", seen.failed.len(), seen.reads, seen.writes, seen.failed.join("\n"));
    // A write in flight as its shard's primary changes may be answered in
    // doubt; a handful at most.
    assert!(seen.in_doubt.len() <= 6, "{} writes in doubt:\n{}", seen.in_doubt.len(), seen.in_doubt.join("\n"));
    assert!(seen.reads > 500 && seen.writes > 100, "{seen:?}");
    // Every write acknowledged is there, once (or overwritten by one in
    // doubt that was kept). A read may go to a replica the last writes
    // have not reached yet (a read that must see a write names it, with
    // min_seq_no): each is read again until it shows, for 5 s at most; a
    // lost write never does.
    let shows = |sql: String, ok: Box<dyn Fn(i64) -> bool>| {
        let run = &run;
        async move {
            let t0 = Instant::now();
            loop {
                let now = run.sql(&sql).await["rows"][0][0].as_i64().unwrap();
                if ok(now) || t0.elapsed() > Duration::from_secs(5) {
                    return now;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    for (key, v) in &seen.written {
        let (v, doubted) = (*v, seen.doubted.get(key).cloned().unwrap_or_default());
        let d = doubted.clone();
        let now = shows(format!("SELECT sold FROM products WHERE id = '{key}'"), Box::new(move |n| n == v || d.contains(&n))).await;
        assert!(now == v || doubted.contains(&now), "{key}: {now}, acknowledged {v}, in doubt {doubted:?}");
    }
    let (lo, hi) = (counted + seen.added, counted + seen.added + seen.added_in_doubt);
    let n = shows(format!("SELECT sold FROM products WHERE id = '{COUNTER}'"), Box::new(move |n| (lo..=hi).contains(&n))).await - counted;
    assert!((seen.added..=seen.added + seen.added_in_doubt).contains(&n), "{COUNTER} went up {n}: {} increments acknowledged, {} in doubt", seen.added, seen.added_in_doubt);
    println!(
        "{} reads and {} writes through three versions, none failed, {} answered in doubt; the slowest took {:.0} ms",
        seen.reads,
        seen.writes,
        seen.in_doubt.len(),
        seen.slowest_ms
    );
}
