//! The network between the cluster's servers: a proxy in front of each
//! server's gRPC port, which can delay what crosses it or drop it, and on
//! Linux firewall rules (iptables, by the uid each process runs as) that cut
//! one process off from another, one way or both.
//!
//! The cluster map names the proxies' ports, so every call between servers
//! and from a coordinator to a server crosses a proxy, and a rule that
//! drops a process's packets to a proxy's port cuts that process off from
//! the server behind it, whatever else reaches it.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

/// What one proxy does to the bytes that cross it.
pub struct Link {
    /// Added to every chunk, each way.
    pub delay_ms: AtomicU64,
    /// Refuse connections, and end those open.
    pub down: AtomicBool,
    /// Bumped to end every connection open.
    cut: watch::Sender<u64>,
    pub connections: AtomicU64,
}

impl Link {
    fn new() -> Arc<Link> {
        Arc::new(Link { delay_ms: AtomicU64::new(0), down: AtomicBool::new(false), cut: watch::channel(0).0, connections: AtomicU64::new(0) })
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
        if down {
            self.cut.send_modify(|n| *n += 1);
        }
    }
}

/// Proxies by the port they listen on.
#[derive(Default)]
pub struct Proxies {
    links: Mutex<HashMap<u16, Arc<Link>>>,
}

impl Proxies {
    /// Serves `listen` in front of `target` until the process ends.
    pub async fn start(&self, listen: u16, target: u16) -> std::io::Result<Arc<Link>> {
        let l = TcpListener::bind(("127.0.0.1", listen)).await?;
        let link = Link::new();
        self.links.lock().unwrap().insert(listen, link.clone());
        let lk = link.clone();
        tokio::spawn(async move {
            loop {
                let Ok((inbound, _)) = l.accept().await else { continue };
                if lk.down.load(Ordering::SeqCst) {
                    drop(inbound);
                    continue;
                }
                let lk = lk.clone();
                tokio::spawn(async move {
                    let Ok(outbound) = TcpStream::connect(("127.0.0.1", target)).await else { return };
                    let _ = inbound.set_nodelay(true);
                    let _ = outbound.set_nodelay(true);
                    lk.connections.fetch_add(1, Ordering::Relaxed);
                    pipe(inbound, outbound, &lk).await;
                    lk.connections.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });
        Ok(link)
    }

    pub fn link(&self, port: u16) -> Option<Arc<Link>> {
        self.links.lock().unwrap().get(&port).cloned()
    }
}

/// Copies both ways until either side closes or the link is cut, each
/// chunk held for the link's delay (read when the chunk arrives), in order.
async fn pipe(a: TcpStream, b: TcpStream, link: &Arc<Link>) {
    let (ar, aw) = a.into_split();
    let (br, bw) = b.into_split();
    let mut cut = link.cut.subscribe();
    let one = one_way(ar, bw, link.clone());
    let two = one_way(br, aw, link.clone());
    tokio::select! {
        _ = one => {}
        _ = two => {}
        _ = cut.changed() => {}
    }
}

async fn one_way(mut r: tokio::net::tcp::OwnedReadHalf, mut w: tokio::net::tcp::OwnedWriteHalf, link: Arc<Link>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    let reader = async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match r.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let due = Instant::now() + Duration::from_millis(link.delay_ms.load(Ordering::Relaxed));
                    if tx.send((due, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    };
    let writer = async move {
        while let Some((due, bytes)) = rx.recv().await {
            let now = Instant::now();
            if due > now {
                tokio::time::sleep(due - now).await;
            }
            if w.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = w.shutdown().await;
    };
    // The writer drains what the reader sent before the reader ended.
    let (_, ()) = tokio::join!(reader, writer);
}

/// Firewall rules in a chain of their own, `STRATA_CHAOS`, jumped to from
/// `OUTPUT`: each drops the TCP packets that processes of one uid send to
/// one port on 127.0.0.1. Only on Linux, as root.
pub struct Firewall {
    pub enabled: bool,
    /// The rules each fault added, to delete when it heals.
    rules: Mutex<HashMap<String, Vec<(u32, u16)>>>,
}

const CHAIN: &str = "STRATA_CHAOS";

impl Firewall {
    /// Sets up the chain, empty; disabled where iptables does not work.
    pub fn new(wanted: bool) -> Firewall {
        let mut enabled = wanted && cfg!(target_os = "linux") && unsafe { libc::geteuid() } == 0;
        if enabled {
            let _ = iptables(&["-N", CHAIN]);
            enabled = iptables(&["-F", CHAIN]).is_ok() && (iptables(&["-C", "OUTPUT", "-j", CHAIN]).is_ok() || iptables(&["-I", "OUTPUT", "-j", CHAIN]).is_ok());
        }
        Firewall { enabled, rules: Mutex::default() }
    }

    /// Drops what processes of `uid` send to `port`, until `heal(id)`.
    pub fn block(&self, id: &str, uid: u32, port: u16) -> Result<(), String> {
        if !self.enabled {
            return Err("cutting links needs iptables (Linux, as root); this run has none".into());
        }
        iptables(&rule("-A", uid, port))?;
        self.rules.lock().unwrap().entry(id.to_string()).or_default().push((uid, port));
        Ok(())
    }

    pub fn heal(&self, id: &str) {
        let rules = self.rules.lock().unwrap().remove(id).unwrap_or_default();
        for (uid, port) in rules {
            let _ = iptables(&rule("-D", uid, port));
        }
    }

    /// Removes the chain and the jump to it.
    pub fn teardown(&self) {
        if self.enabled {
            let _ = iptables(&["-D", "OUTPUT", "-j", CHAIN]);
            let _ = iptables(&["-F", CHAIN]);
            let _ = iptables(&["-X", CHAIN]);
        }
    }
}

fn rule(op: &str, uid: u32, port: u16) -> Vec<String> {
    let (uid, port) = (uid.to_string(), port.to_string());
    [op, CHAIN, "-p", "tcp", "-d", "127.0.0.1", "--dport", &port, "-m", "owner", "--uid-owner", &uid, "-j", "DROP"].iter().map(|s| s.to_string()).collect()
}

fn iptables<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Result<(), String> {
    let out = Command::new("iptables").args(args).output().map_err(|e| format!("iptables: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("iptables: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
