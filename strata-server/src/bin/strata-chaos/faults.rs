//! The faults the monkey injects, each undone by `heal` (or by itself, for
//! a crash the supervisor recovers from), and the targets they name.
//!
//! A target is a process's name (`node4`, `coord1`) or a role
//! resolved against the cluster map the coordinators route by now:
//! `primary:N`, `replica:N` (shard N's first replica), `replicas:N`,
//! `shard:N` (every copy), `coord` (the first coordinator that runs),
//! `any-copy` and `any-replica` (one drawn at random).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::{json, Value as Json};

use crate::cluster::{Cluster, Kind};
use crate::net::{Firewall, Proxies};
use crate::observe::Seen;
use crate::workload::Workload;

/// Every fault: its name, what it takes, and what it stands for.
pub const CATALOG: &[(&str, &str, &str)] = &[
    ("kill", "TARGET", "SIGKILL; the supervisor starts it again after --restart-after-ms (a crash; Kubernetes restarts the pod)"),
    ("stop", "TARGET", "kill it and keep it down until healed (a lost node, a pod that cannot be scheduled)"),
    ("pause", "TARGET [--ms N]", "SIGSTOP until healed, or for N ms (a frozen VM, a long stall)"),
    ("slow-cpu", "TARGET [--pct 80]", "stopped for pct% of every 100 ms (CPU throttling, a noisy neighbour)"),
    ("delay", "TARGET [--ms 200]", "every byte to and from its gRPC port waits N ms (a slow network to it)"),
    ("isolate", "TARGET", "cut it off from every other server and coordinator, both ways (iptables)"),
    ("cut", "TARGET --to TARGET [--oneway]", "cut the calls from one to the other, and back unless --oneway (iptables)"),
    ("disk-fill", "TARGET [--free-mb 1]", "a ballast file leaves N MiB free on its disk (needs --disk-mb)"),
    ("corrupt", "TARGET", "flip bytes in its largest data file, then kill it so it opens the damaged file (bit rot)"),
    ("wipe", "TARGET", "stop it, delete its data and start it empty, as a spare starts (a pod rescheduled without its volume)"),
    ("bad-token", "TARGET", "start it again with a wrong --cluster-token (a bad configuration pushed to one server)"),
    ("flood", "[--clients 32]", "a client with its own key sends heavy queries without pause (a runaway report job)"),
    ("hot-shard", "[--shard 0] [--clients 8]", "writers add to a few keys of one shard as fast as they are answered (a hot partition)"),
];

/// What a fault changed, to undo it.
enum Undo {
    Nothing,
    Release(Vec<String>),
    Cont(Vec<String>),
    StopTasks(Arc<AtomicBool>, Vec<String>),
    Delay(Vec<u16>),
    Firewall,
    Ballast(PathBuf),
    Token(String),
}

struct Active {
    fault: String,
    targets: Vec<String>,
    t: f64,
    undo: Undo,
    detail: Json,
}

pub struct Monkey {
    pub cluster: Arc<Cluster>,
    pub proxies: Arc<Proxies>,
    pub firewall: Arc<Firewall>,
    pub work: Arc<Workload>,
    pub seen: Arc<Seen>,
    active: Mutex<BTreeMap<String, Active>>,
    n: AtomicU64,
}

/// A request to inject a fault.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Inject {
    pub fault: String,
    pub target: Option<String>,
    pub to: Option<String>,
    pub oneway: bool,
    pub ms: Option<u64>,
    pub pct: Option<u64>,
    pub free_mb: Option<u64>,
    pub clients: Option<usize>,
    pub shard: Option<usize>,
}

impl Monkey {
    pub fn new(cluster: Arc<Cluster>, proxies: Arc<Proxies>, firewall: Arc<Firewall>, work: Arc<Workload>, seen: Arc<Seen>) -> Arc<Monkey> {
        Arc::new(Monkey { cluster, proxies, firewall, work, seen, active: Mutex::default(), n: AtomicU64::new(0) })
    }

    /// The processes a target names, now.
    pub fn resolve(&self, target: &str) -> Result<Vec<String>, String> {
        let look = self.seen.last.lock().unwrap().clone();
        let shard = |s: &str| -> Result<Vec<String>, String> {
            let i: usize = s.parse().map_err(|_| format!("{target}: not a shard number"))?;
            look.shards.get(i).cloned().filter(|v| !v.is_empty()).ok_or_else(|| format!("{target}: the map has no shard {i} (yet)"))
        };
        let mut rng = rand::thread_rng();
        let names = match target.split_once(':') {
            Some(("primary", s)) => {
                let i: usize = s.parse().map_err(|_| format!("{target}: not a shard number"))?;
                vec![look.primaries.get(i).cloned().flatten().ok_or_else(|| format!("{target}: shard {i} has no primary now"))?]
            }
            Some(("replica", s)) => vec![shard(s)?.into_iter().skip(1).next().ok_or_else(|| format!("{target}: no replica"))?],
            Some(("replicas", s)) => shard(s)?.into_iter().skip(1).collect(),
            Some(("shard", s)) => shard(s)?,
            _ if target == "coord" => vec![self.cluster.procs.lock().unwrap().iter().find(|p| p.kind == Kind::Coordinator && p.running()).map(|p| p.name.clone()).ok_or("no coordinator runs")?],
            _ if target == "any-copy" => vec![look.shards.iter().flatten().cloned().collect::<Vec<_>>().choose(&mut rng).cloned().ok_or("no copies")?],
            _ if target == "any-replica" => vec![look.shards.iter().flat_map(|s| s.iter().skip(1)).cloned().collect::<Vec<_>>().choose(&mut rng).cloned().ok_or("no replicas")?],
            _ => {
                self.cluster.with(target, |_| ())?;
                vec![target.to_string()]
            }
        };
        Ok(names)
    }

    pub fn active(&self) -> Json {
        let a = self.active.lock().unwrap();
        Json::Array(a.iter().map(|(id, f)| json!({"id": id, "fault": f.fault, "targets": f.targets, "since_s": (self.cluster.events.t() - f.t).round(), "detail": f.detail})).collect())
    }

    pub fn active_targets(&self) -> Vec<String> {
        self.active.lock().unwrap().values().flat_map(|a| a.targets.clone()).collect()
    }

    pub fn inject(self: &Arc<Self>, r: &Inject) -> Result<Json, String> {
        let id = format!("f{}", self.n.fetch_add(1, Ordering::Relaxed) + 1);
        let targets = match r.fault.as_str() {
            "flood" => Vec::new(),
            "hot-shard" => Vec::new(),
            _ => self.resolve(r.target.as_deref().ok_or("this fault needs a target")?)?,
        };
        let c = &self.cluster;
        let mut detail = json!({});
        let undo = match r.fault.as_str() {
            "kill" => {
                for t in &targets {
                    c.signal(t, libc::SIGKILL)?;
                }
                Undo::Nothing
            }
            "stop" => {
                for t in &targets {
                    c.hold(t, true)?;
                }
                Undo::Release(targets.clone())
            }
            "pause" => {
                for t in &targets {
                    c.signal(t, libc::SIGSTOP)?;
                }
                if let Some(ms) = r.ms {
                    let me = self.clone();
                    let id2 = id.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        let _ = me.heal(&id2);
                    });
                }
                Undo::Cont(targets.clone())
            }
            "slow-cpu" => {
                let pct = r.pct.unwrap_or(80).clamp(1, 99);
                let stop = Arc::new(AtomicBool::new(false));
                for t in &targets {
                    let (c2, t2, stop2) = (c.clone(), t.clone(), stop.clone());
                    std::thread::spawn(move || {
                        while !stop2.load(Ordering::Relaxed) {
                            let _ = c2.signal(&t2, libc::SIGSTOP);
                            std::thread::sleep(Duration::from_millis(pct));
                            let _ = c2.signal(&t2, libc::SIGCONT);
                            std::thread::sleep(Duration::from_millis(100 - pct));
                        }
                        let _ = c2.signal(&t2, libc::SIGCONT);
                    });
                }
                detail = json!({"pct": pct});
                Undo::StopTasks(stop, targets.clone())
            }
            "delay" => {
                let ms = r.ms.unwrap_or(200);
                let mut ports = Vec::new();
                for t in &targets {
                    let port = c.with(t, |p| p.proxy)?;
                    let link = self.proxies.link(port).ok_or_else(|| format!("{t} has no proxy"))?;
                    link.delay_ms.store(ms, Ordering::SeqCst);
                    ports.push(port);
                }
                detail = json!({"ms": ms});
                Undo::Delay(ports)
            }
            "isolate" => {
                let all: Vec<(String, u32, u16)> = c.procs.lock().unwrap().iter().map(|p| (p.name.clone(), p.uid, p.proxy)).collect();
                for t in &targets {
                    let (tuid, tport) = c.with(t, |p| (p.uid, p.proxy))?;
                    for (n, uid, port) in &all {
                        if targets.contains(n) {
                            continue;
                        }
                        // Its calls out, and every call in.
                        if *port > 0 {
                            self.firewall.block(&id, tuid, *port)?;
                        }
                        self.firewall.block(&id, *uid, tport)?;
                    }
                }
                Undo::Firewall
            }
            "cut" => {
                let to = self.resolve(r.to.as_deref().ok_or("cut needs --to")?)?;
                for a in &targets {
                    for b in &to {
                        let (auid, aport) = c.with(a, |p| (p.uid, p.proxy))?;
                        let (buid, bport) = c.with(b, |p| (p.uid, p.proxy))?;
                        if bport > 0 {
                            self.firewall.block(&id, auid, bport)?;
                        }
                        if !r.oneway && aport > 0 {
                            self.firewall.block(&id, buid, aport)?;
                        }
                    }
                }
                detail = json!({"to": to, "oneway": r.oneway});
                Undo::Firewall
            }
            "disk-fill" => {
                if c.layout.disk_mb == 0 {
                    return Err("disk-fill needs servers on disks of their own: start with --disk-mb".into());
                }
                let t = targets.first().ok_or("no target")?;
                let dir = c.with(t, |p| p.dir.clone())?;
                let free_mb = r.free_mb.unwrap_or(1);
                let path = dir.join("ballast.bin");
                let free = free_bytes(&dir)?;
                let size = free.saturating_sub(free_mb * 1024 * 1024);
                fill(&path, size)?;
                detail = json!({"ballast_mb": size / (1024 * 1024), "free_mb": free_mb});
                Undo::Ballast(path)
            }
            "corrupt" => {
                let t = targets.first().ok_or("no target")?;
                let files = c.segment_files(t);
                let file = files.first().ok_or_else(|| format!("{t} has no data files"))?.clone();
                let flipped = flip_bytes(&file)?;
                detail = json!({"file": file.display().to_string(), "flipped_at": flipped});
                c.signal(t, libc::SIGKILL)?;
                Undo::Nothing
            }
            "wipe" => {
                let t = targets.first().ok_or("no target")?.clone();
                c.hold(&t, true)?;
                let dir = c.with(&t, |p| p.dir.clone())?;
                for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
                    let p = e.path();
                    let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                }
                c.with(&t, |p| {
                    p.as_spare = true;
                    p.initialized = false;
                })?;
                c.hold(&t, false)?;
                Undo::Nothing
            }
            "bad-token" => {
                let t = targets.first().ok_or("no target")?.clone();
                c.with(&t, |p| p.cluster_token = Some(format!("wrong-{}", rand::thread_rng().gen::<u32>())))?;
                c.signal(&t, libc::SIGTERM)?;
                Undo::Token(t)
            }
            "flood" => {
                let n = r.clients.unwrap_or(32);
                let stop = Arc::new(AtomicBool::new(false));
                for _ in 0..n {
                    tokio::spawn(self.work.clone().flooder(stop.clone(), FLOOD_SQL.to_string()));
                }
                detail = json!({"clients": n, "sql": FLOOD_SQL, "key": "nightly-report"});
                Undo::StopTasks(stop, Vec::new())
            }
            "hot-shard" => {
                let shard = r.shard.unwrap_or(0);
                let n = r.clients.unwrap_or(8);
                let keys = self.keys_of_shard(shard, 8)?;
                let stop = Arc::new(AtomicBool::new(false));
                for i in 0..n {
                    tokio::spawn(self.work.clone().hot_writer(stop.clone(), keys.clone(), i));
                }
                detail = json!({"shard": shard, "clients": n, "keys": keys});
                Undo::StopTasks(stop, Vec::new())
            }
            f => return Err(format!("no fault named {f}: strata-chaos faults lists them")),
        };
        let text = format!("{id} {}{}{}", r.fault, if targets.is_empty() { String::new() } else { format!(" {}", targets.join(",")) }, if detail.as_object().is_some_and(|o| o.is_empty()) { String::new() } else { format!(" {detail}") });
        c.events.add("fault", &text, json!({"id": id, "fault": r.fault, "targets": targets, "detail": detail}));
        let heals_itself = matches!(undo, Undo::Nothing);
        if !heals_itself {
            self.active.lock().unwrap().insert(id.clone(), Active { fault: r.fault.clone(), targets: targets.clone(), t: c.events.t(), undo, detail: detail.clone() });
        }
        Ok(json!({"id": id, "fault": r.fault, "targets": targets, "detail": detail, "active": !heals_itself}))
    }

    /// Undoes a fault.
    pub fn heal(&self, id: &str) -> Result<Json, String> {
        let a = self.active.lock().unwrap().remove(id).ok_or_else(|| format!("no active fault {id}"))?;
        let c = &self.cluster;
        match &a.undo {
            Undo::Nothing => {}
            Undo::Release(ts) => {
                for t in ts {
                    c.hold(t, false)?;
                }
            }
            Undo::Cont(ts) => {
                for t in ts {
                    let _ = c.signal(t, libc::SIGCONT);
                }
            }
            Undo::StopTasks(stop, ts) => {
                stop.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(250));
                for t in ts {
                    let _ = c.signal(t, libc::SIGCONT);
                }
            }
            Undo::Delay(ports) => {
                for p in ports {
                    if let Some(l) = self.proxies.link(*p) {
                        l.delay_ms.store(0, Ordering::SeqCst);
                    }
                }
            }
            Undo::Firewall => self.firewall.heal(id),
            Undo::Ballast(p) => {
                let _ = std::fs::remove_file(p);
            }
            Undo::Token(t) => {
                c.with(t, |p| p.cluster_token = None)?;
                c.signal(t, libc::SIGTERM)?;
            }
        }
        c.events.add("heal", &format!("{id} {} healed after {:.0} s", a.fault, c.events.t() - a.t), json!({"id": id, "fault": a.fault, "targets": a.targets}));
        Ok(json!({"healed": id, "fault": a.fault}))
    }

    pub fn heal_all(&self) -> Vec<String> {
        let ids: Vec<String> = self.active.lock().unwrap().keys().cloned().collect();
        for id in &ids {
            let _ = self.heal(id);
        }
        ids
    }

    /// Keys of the counter table whose slots shard `shard` holds now.
    fn keys_of_shard(&self, shard: usize, n: usize) -> Result<Vec<String>, String> {
        let ranges = self.seen.slot_ranges.lock().unwrap().get(shard).cloned().ok_or_else(|| format!("no slots known for shard {shard}"))?;
        let mut keys = Vec::new();
        for i in 0..100_000 {
            let k = format!("chaos-hot-{i}");
            let s = strata_server::slots::slot(&strata::Value::Str(k.clone()));
            if ranges.iter().any(|&(a, b)| s >= a && s < b) {
                keys.push(k);
                if keys.len() == n {
                    break;
                }
            }
        }
        Ok(keys)
    }
}

/// The flood's query: a grouping over every row that matches common
/// words, which no cache answers twice (the words change).
pub const FLOOD_SQL: &str = "SELECT category, brand, COUNT(*) AS n, AVG(price) AS price, MAX(rating) AS best FROM products WHERE MATCH(body, 'steel OR wool OR cotton OR portable OR quiet OR ceramic OR leather OR bamboo') GROUP BY category, brand ORDER BY n DESC LIMIT 100";

fn free_bytes(dir: &std::path::Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(st.f_bavail as u64 * st.f_frsize as u64)
}

fn fill(path: &std::path::Path, size: u64) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let chunk = vec![0x5au8; 1 << 20];
    let mut left = size;
    while left > 0 {
        let n = left.min(chunk.len() as u64) as usize;
        if f.write_all(&chunk[..n]).is_err() {
            break;
        }
        left -= n as u64;
    }
    let _ = f.sync_all();
    Ok(())
}

/// Flips 64 bytes at three places past the file's first 4 KiB.
fn flip_bytes(path: &std::path::Path) -> Result<Vec<u64>, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    if len < 8192 {
        return Err(format!("{} is too small to damage ({len} bytes)", path.display()));
    }
    let mut rng = rand::thread_rng();
    let mut at = Vec::new();
    for _ in 0..3 {
        let off = rng.gen_range(4096..len - 64);
        let mut buf = [0u8; 64];
        f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
        f.read_exact(&mut buf).map_err(|e| e.to_string())?;
        for b in &mut buf {
            *b ^= 0xff;
        }
        f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
        f.write_all(&buf).map_err(|e| e.to_string())?;
        at.push(off);
    }
    f.sync_all().map_err(|e| e.to_string())?;
    Ok(at)
}
