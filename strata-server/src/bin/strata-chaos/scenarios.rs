//! Scenarios: one fault each, as an operator meets it, with what brings
//! the cluster back (what Kubernetes does by itself, or the admin call
//! the command center's button makes), and what each did:
//!
//! - the clients' calls while it lasted (failures by kind and class,
//!   latency, for how long writes or searches failed);
//! - when the command center first showed it (Home's status and headline,
//!   its problems, the alerts), and when that cleared;
//! - whether the counters still hold every acknowledged write, once.
//!
//! `monkey` instead draws faults at random for a while.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rand::{Rng, SeedableRng};
use serde_json::{json, Value as Json};

use crate::faults::Inject;
use crate::observe::Look;
use crate::World;

pub struct Scenario {
    pub name: &'static str,
    pub title: &'static str,
}

const LIST: &[(&str, &str)] = &[
    ("replica-crash", "A replica's process crashes and its pod starts again"),
    ("primary-crash", "A primary's process crashes and its pod starts again"),
    ("replica-lost", "A replica's node is lost for 90 s"),
    ("primary-lost", "A primary's node is lost for 90 s"),
    ("majority-lost", "Two of a shard's three copies are lost for 60 s"),
    ("shard-down", "Every copy of a shard is down for 45 s"),
    ("primary-pause", "A primary freezes for 20 s (a stalled VM), then wakes up"),
    ("primary-isolated", "A primary is cut off from the cluster for 45 s"),
    ("coordinator-cut", "A coordinator cannot reach one replica (one way) for 60 s"),
    ("slow-network", "The network to a primary adds 300 ms each way for 60 s"),
    ("slow-replica", "The network to one replica adds 300 ms each way for 60 s"),
    ("slow-cpu", "A primary gets 15% of its CPU for 60 s (throttled)"),
    ("disk-full", "A primary's disk fills up for 60 s"),
    ("bit-rot", "A replica's data file is damaged on disk, and it restarts"),
    ("lost-volume", "A replica comes back without its volume"),
    ("bad-config", "A replica restarts with the wrong cluster token for 60 s"),
    ("query-flood", "A report job floods the cluster with heavy queries for 60 s"),
    ("hot-shard", "Writers hammer a few keys of one shard for 60 s"),
    ("coordinator-crash", "A coordinator crashes and starts again"),
    ("move-interrupted", "A coordinator dies while it moves slots to a new shard"),
];

pub fn all() -> Vec<Scenario> {
    LIST.iter().map(|&(name, title)| Scenario { name, title }).collect()
}

fn t(w: &World) -> f64 {
    w.events.t()
}

async fn sleep(s: f64) {
    tokio::time::sleep(Duration::from_secs_f64(s)).await;
}

fn inject(w: &World, fault: &str, target: &str) -> Result<String, String> {
    inject_with(w, Inject { fault: fault.into(), target: Some(target.into()), ..Inject::default() })
}

fn inject_with(w: &World, r: Inject) -> Result<String, String> {
    let j = w.monkey.inject(&r)?;
    Ok(j["id"].as_str().unwrap_or("").to_string())
}

fn heal(w: &World, id: &str) -> Result<(), String> {
    w.monkey.heal(id).map(|_| ())
}

fn look(w: &World) -> Look {
    w.seen.last.lock().unwrap().clone()
}

/// Waits until `f` holds of the observer's look, for `secs` at most.
async fn until(w: &World, secs: f64, f: impl Fn(&Look) -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < secs {
        if f(&look(w)) {
            return true;
        }
        sleep(0.5).await;
    }
    false
}

/// The shard a process serves in the map now.
fn shard_of(w: &World, name: &str) -> Option<usize> {
    look(w).shards.iter().position(|s| s.iter().any(|n| n == name))
}

fn grpc(w: &World, name: &str) -> String {
    w.cluster.with(name, |p| p.grpc_url()).unwrap_or_default()
}

/// Whether the cluster is as it should be: every process up, Home all
/// right, the clients' last 10 s without a failure.
fn healthy(w: &World) -> (bool, String) {
    // A copy a scenario removed from its shard and holds down (bit rot)
    // is no longer the cluster's.
    let l = look(w);
    let in_map = |n: &str| l.shards.iter().flatten().any(|m| m == n);
    let down: Vec<String> = w.cluster.procs.lock().unwrap().iter().filter(|p| !p.running() && !(p.held && !matches!(p.kind, crate::cluster::Kind::Coordinator) && !in_map(&p.name))).map(|p| p.name.clone()).collect();
    if !down.is_empty() {
        return (false, format!("down: {}", down.join(", ")));
    }
    // Home may still show alerts of an earlier fault (a primary change is
    // an info alert for 10 minutes): only its other problems count. Nor its
    // disks: the run's own writes and logs fill a small `--disk-mb` disk
    // over an hour, which Home forecasts, and no scenario waits for that.
    let base = BASELINE.lock().unwrap().clone();
    let left: Vec<&String> = l.problems.keys().filter(|k| !matches!(k.as_str(), "alerts" | "error_ratio_recovered" | "p99_recovered" | "disk") && !base.contains(*k)).collect();
    if l.status == "unreachable" || !left.is_empty() {
        return (false, format!("Home: {} — {}", l.status, l.headline));
    }
    let now = w.events.start.elapsed().as_secs();
    let s = w.work.stats.summary(now.saturating_sub(10), now);
    let failed: u64 = ["search", "get", "write"].iter().map(|k| s[k]["failed"].as_u64().unwrap_or(0)).sum();
    if failed > 0 {
        return (false, format!("{failed} client calls failed in the last 10 s"));
    }
    (true, String::new())
}

/// Home's problems before the first fault, which are this machine's own (a
/// disk it already fills, say): the scenarios do not wait for them to clear.
static BASELINE: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Waits for the cluster to start: every process up and the clients 10 s
/// without a failure, then up to 30 s for Home to clear. What Home still
/// shows then is taken as the machine's own, and later waits ignore it.
async fn settle_start(w: &World) -> (bool, String) {
    let t0 = Instant::now();
    loop {
        let (ok, why) = healthy(w);
        let waited = t0.elapsed().as_secs_f64();
        if ok || waited > 90.0 {
            return (ok, why);
        }
        if why.starts_with("Home:") && waited > 30.0 {
            let l = look(w);
            let mine: BTreeSet<String> = l.problems.keys().filter(|k| !matches!(k.as_str(), "alerts" | "error_ratio_recovered" | "p99_recovered" | "disk")).cloned().collect();
            let titles: Vec<&String> = mine.iter().filter_map(|k| l.problems.get(k)).collect();
            w.events.add("note", &format!("before any fault Home shows {}: the scenarios do not wait for it to clear", titles.iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(", ")), json!({"ignored": mine}));
            *BASELINE.lock().unwrap() = mine;
            return (true, String::new());
        }
        sleep(1.0).await;
    }
}

async fn settle(w: &World, secs: f64) -> (bool, String) {
    let t0 = Instant::now();
    loop {
        let (ok, why) = healthy(w);
        if ok || t0.elapsed().as_secs_f64() > secs {
            return (ok, why);
        }
        sleep(1.0).await;
    }
}

/// Plays one scenario: the fault, the wait, and what brings the cluster
/// back. Answers the times of its steps.
async fn play(w: &World, name: &str) -> Result<Json, String> {
    let mut steps = Vec::new();
    let mut step = |w: &World, what: &str| steps.push(json!({"t": (t(w) * 10.0).round() / 10.0, "step": what}));
    match name {
        "replica-crash" | "primary-crash" => {
            let target = if name == "replica-crash" { "replica:0" } else { "primary:0" };
            inject(w, "kill", target)?;
            step(w, "inject");
            sleep(40.0).await;
        }
        "replica-lost" | "primary-lost" => {
            let target = if name == "replica-lost" { "replica:1" } else { "primary:1" };
            let id = inject(w, "stop", target)?;
            step(w, "inject");
            sleep(90.0).await;
            heal(w, &id)?;
            step(w, "heal: the node is back");
        }
        "majority-lost" => {
            let id = inject(w, "stop", "replicas:0")?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: both nodes are back");
        }
        "shard-down" => {
            let id = inject(w, "stop", "shard:1")?;
            step(w, "inject");
            sleep(45.0).await;
            heal(w, &id)?;
            step(w, "heal: the nodes are back");
        }
        "primary-pause" => {
            inject_with(w, Inject { fault: "pause".into(), target: Some("primary:0".into()), ms: Some(20_000), ..Inject::default() })?;
            step(w, "inject");
            sleep(20.0).await;
            step(w, "heal: it wakes up");
            sleep(20.0).await;
        }
        "primary-isolated" => {
            let id = inject(w, "isolate", "primary:1")?;
            step(w, "inject");
            sleep(45.0).await;
            heal(w, &id)?;
            step(w, "heal: the network is back");
            sleep(15.0).await;
        }
        "coordinator-cut" => {
            let id = inject_with(w, Inject { fault: "cut".into(), target: Some("coord0".into()), to: Some("replica:0".into()), oneway: true, ..Inject::default() })?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: the link is back");
        }
        "slow-network" => {
            let id = inject_with(w, Inject { fault: "delay".into(), target: Some("primary:0".into()), ms: Some(300), ..Inject::default() })?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: the network is fast again");
        }
        "slow-replica" => {
            let id = inject_with(w, Inject { fault: "delay".into(), target: Some("replica:0".into()), ms: Some(300), ..Inject::default() })?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: the network is fast again");
        }
        "slow-cpu" => {
            let id = inject_with(w, Inject { fault: "slow-cpu".into(), target: Some("primary:1".into()), pct: Some(85), ..Inject::default() })?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: its CPU is back");
        }
        "disk-full" => {
            let target = w.monkey.resolve("primary:0")?[0].clone();
            let shard = shard_of(w, &target).ok_or("no shard")?;
            let id = inject_with(w, Inject { fault: "disk-fill".into(), target: Some(target.clone()), free_mb: Some(0), ..Inject::default() })?;
            step(w, "inject");
            sleep(40.0).await;
            // The quick fix the Cluster view offers: another copy takes writes.
            let replica = w.monkey.resolve(&format!("replica:{shard}"))?[0].clone();
            let r = w.admin(reqwest::Method::POST, &format!("/v1/admin/shards/{shard}/promote"), Some(json!({"url": grpc(w, &replica)}))).await;
            step(w, &format!("fix: promote {replica}: {}", short(&r)));
            sleep(20.0).await;
            heal(w, &id)?;
            step(w, "fix: free the disk (the ballast is deleted)");
            // The alert's advice: restart the server once the cause is fixed.
            w.cluster.signal(&target, libc::SIGTERM)?;
            step(w, &format!("fix: restart {target}, as the 'Database refuses writes' alert says"));
            sleep(15.0).await;
        }
        "bit-rot" => {
            let target = w.monkey.resolve("replica:0")?[0].clone();
            let shard = shard_of(w, &target).ok_or("no shard")?;
            inject(w, "corrupt", &target)?;
            step(w, "inject");
            sleep(45.0).await;
            // What the cluster's own check finds: the copies' digests.
            let verify = w.admin(reqwest::Method::POST, "/v1/admin/copies/verify", Some(json!({"url": grpc(w, &target)}))).await;
            let differ: u64 = verify.as_ref().ok().and_then(|v| v["replicas"].as_array()).into_iter().flatten().map(|r| r["differing_slots"].as_u64().unwrap_or(0)).sum();
            step(w, &format!("verify the copies (Cluster view): {} slots differ", differ));
            // The checksum check: from the copy's menu ("Check its data"),
            // or, on a coordinator without it, from the server's own UI.
            let damaged = match w.admin(reqwest::Method::POST, "/v1/admin/copies/scrub", Some(json!({"url": grpc(w, &target)}))).await {
                Ok(j) => {
                    let found: Vec<&str> = j["scrub"]["damaged"].as_array().into_iter().flatten().filter_map(Json::as_str).collect();
                    step(w, &format!("check its data (the copy's menu): {} damaged {}", found.len(), found.join("; ")));
                    // Home names the damaged copy before it is replaced.
                    sleep(5.0).await;
                    !found.is_empty()
                }
                Err(e) if e.starts_with("404") => {
                    let url = w.cluster.with(&target, |p| p.http_url())?;
                    let r = w.http.post(format!("{url}/v1/admin/check")).header("authorization", format!("Bearer {}", w.cluster.layout.cluster_token)).timeout(Duration::from_secs(120)).send().await.map_err(|e| e.to_string())?;
                    let body = r.text().await.unwrap_or_default();
                    step(w, &format!("check the database on {target} itself: {}", body.chars().take(200).collect::<String>()));
                    body.contains("corrupt")
                }
                Err(e) => {
                    step(w, &format!("check its data: {e}"));
                    false
                }
            };
            if damaged || differ > 0 {
                replace_copy(w, &mut step, shard, &target).await?;
            }
        }
        "lost-volume" => {
            let target = w.monkey.resolve("replica:1")?[0].clone();
            let shard = shard_of(w, &target).ok_or("no shard")?;
            inject(w, "wipe", &target)?;
            step(w, "inject");
            sleep(45.0).await;
            // The operator's fix for a pod back without its volume: it
            // leaves its shard and joins again, and is sent a copy.
            let url = grpc(w, &target);
            let r = w.admin(reqwest::Method::DELETE, &format!("/v1/admin/shards/{shard}/replicas?url={}", urlencode(&url)), None).await;
            step(w, &format!("fix: remove it from shard {shard}: {}", short(&r)));
            let r = w.admin(reqwest::Method::POST, &format!("/v1/admin/shards/{shard}/replicas"), Some(json!({"url": url}))).await;
            step(w, &format!("fix: add it again: {}", short(&r)));
            sleep(20.0).await;
        }
        "bad-config" => {
            let id = inject(w, "bad-token", "replica:0")?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: the right token, restarted");
        }
        "query-flood" => {
            let id = inject_with(w, Inject { fault: "flood".into(), clients: Some(32), ..Inject::default() })?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: the job stops");
        }
        "hot-shard" => {
            let id = inject_with(w, Inject { fault: "hot-shard".into(), shard: Some(0), clients: Some(8), ..Inject::default() })?;
            step(w, "inject");
            sleep(60.0).await;
            heal(w, &id)?;
            step(w, "heal: the writers stop");
        }
        "coordinator-crash" => {
            inject(w, "kill", "coord0")?;
            step(w, "inject");
            sleep(30.0).await;
        }
        "move-interrupted" => {
            let in_map: BTreeSet<String> = look(w).shards.iter().flatten().cloned().collect();
            let spare = w.cluster.procs.lock().unwrap().iter().find(|p| p.kind == crate::cluster::Kind::Spare && !in_map.contains(&p.name) && p.running()).map(|p| p.name.clone()).ok_or("no spare left: an earlier scenario put each in a shard")?;
            let url = grpc(w, &spare);
            let r = w.admin(reqwest::Method::POST, "/v1/admin/shards", Some(json!({"servers": [url]}))).await?;
            step(w, &format!("add {spare} as a shard: {}", r.to_string().chars().take(200).collect::<String>()));
            let shards = look(w).shards.len();
            let _ = until(w, 15.0, |l| l.shards.len() > shards).await;
            // On coord0, whose death interrupts it.
            let coord0 = w.cluster.with("coord0", |p| p.http_url())?;
            // Small chunks, so that the job outlasts the half second
            // before the kill: a slot of the demo's rows moves in well
            // under a millisecond.
            let r = w.http.post(format!("{coord0}/v1/admin/rebalance")).header("authorization", format!("Bearer {}", w.cluster.layout.admin_token)).json(&json!({"chunk": 8})).send().await.map_err(|e| e.to_string())?;
            step(w, &format!("rebalance on coord0: {}", r.status()));
            sleep(0.5).await;
            let id = inject(w, "stop", "coord0")?;
            step(w, "inject: coord0 dies during the move");
            sleep(20.0).await;
            heal(w, &id)?;
            step(w, "heal: coord0 is back");
            sleep(10.0).await;
            let moves = w.admin(reqwest::Method::GET, "/v1/admin/moves", None).await.unwrap_or(Json::Null);
            if !moves["moving"].is_null() {
                let r = w.admin(reqwest::Method::POST, "/v1/admin/moves/finish", Some(json!({}))).await;
                step(w, &format!("fix: finish the move left in the map: {}", short(&r)));
                sleep(10.0).await;
            }
            let r = w.admin(reqwest::Method::POST, "/v1/admin/rebalance", Some(json!({}))).await;
            step(w, &format!("fix: rebalance again: {}", short(&r)));
            // Until the job ends.
            for _ in 0..120 {
                let m = w.admin(reqwest::Method::GET, "/v1/admin/moves", None).await.unwrap_or(Json::Null);
                if m["state"].as_str() != Some("running") && m["moving"].is_null() {
                    step(w, &format!("moves: {}", m["state"].as_str().unwrap_or("?")));
                    break;
                }
                sleep(1.0).await;
            }
        }
        n => return Err(format!("no scenario named {n}")),
    }
    Ok(Json::Array(steps))
}

/// Takes a broken copy out of its shard and puts a spare in its place,
/// as the command center's Cluster view does it: add the spare as a
/// replica, then remove the broken copy.
async fn replace_copy(w: &World, step: &mut impl FnMut(&World, &str), shard: usize, broken: &str) -> Result<(), String> {
    let in_map: BTreeSet<String> = look(w).shards.iter().flatten().cloned().collect();
    let spare = w.cluster.procs.lock().unwrap().iter().find(|p| p.kind == crate::cluster::Kind::Spare && !in_map.contains(&p.name) && p.running()).map(|p| p.name.clone()).ok_or("no spare left")?;
    let r = w.admin(reqwest::Method::POST, &format!("/v1/admin/shards/{shard}/replicas"), Some(json!({"url": grpc(w, &spare)}))).await;
    step(w, &format!("fix: add {spare} to shard {shard}: {}", short(&r)));
    let r = w.admin(reqwest::Method::DELETE, &format!("/v1/admin/shards/{shard}/replicas?url={}", urlencode(&grpc(w, broken))), None).await;
    step(w, &format!("fix: remove {broken}: {}", short(&r)));
    // The broken copy is out of the cluster: its process stops.
    let _ = w.cluster.hold(broken, true);
    sleep(20.0).await;
    Ok(())
}

fn short(r: &Result<Json, String>) -> String {
    match r {
        Ok(j) => format!("ok {}", j.to_string().chars().take(160).collect::<String>()),
        Err(e) => format!("refused: {}", e.chars().take(240).collect::<String>()),
    }
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// When the command center first showed something it did not show before
/// `t0`, and what; and when it was all right again.
fn detection(w: &World, t0: f64, base: &Look, end: f64) -> Json {
    let looks: Vec<Look> = w.seen.looks.lock().unwrap().iter().filter(|l| l.t >= t0 && l.t <= end).cloned().collect();
    let differs = |l: &Look| l.status != base.status || l.problems.keys().any(|k| !base.problems.contains_key(k)) || l.alerts.keys().any(|k| !base.alerts.contains_key(k));
    let first = looks.iter().find(|l| differs(l));
    let first_alert = looks.iter().find(|l| l.alerts.keys().any(|k| !base.alerts.contains_key(k)));
    let worst = looks.iter().max_by_key(|l| match l.status.as_str() { "critical" => 3, "unreachable" => 3, "warning" => 2, "ok" => 1, _ => 0 });
    let mut problems = BTreeSet::new();
    let mut alerts = BTreeSet::new();
    let mut headlines: Vec<String> = Vec::new();
    for l in &looks {
        for (k, v) in &l.problems {
            if !base.problems.contains_key(k) {
                problems.insert(format!("{k}: {v}"));
            }
        }
        for (k, v) in &l.alerts {
            if !base.alerts.contains_key(k) {
                alerts.insert(v.clone());
            }
            let _ = k;
        }
        if l.status != "ok" && !headlines.contains(&l.headline) && headlines.len() < 6 {
            headlines.push(l.headline.clone());
        }
    }
    let cleared = first.and_then(|f| looks.iter().filter(|l| l.t > f.t).find(|l| l.status == "ok" && !differs(l))).map(|l| l.t);
    json!({
        "first_signal_s": first.map(|l| round1(l.t - t0)),
        "first_signal": first.map(|l| format!("{}: {}", l.status, l.headline)),
        "first_alert_s": first_alert.map(|l| round1(l.t - t0)),
        "worst": worst.map(|l| format!("{}: {}", l.status, l.headline)),
        "headlines": headlines,
        "problems": problems,
        "alerts": alerts,
        "home_ok_again_s": cleared.map(|c| round1(c - t0)),
    })
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The first second from which the clients saw no failure for 10 s.
fn recovered_at(w: &World, from: u64, to: u64) -> Option<u64> {
    let s = w.work.stats.series(from, to);
    let rows = s.as_array()?;
    let mut clean = 0;
    let mut start = None;
    for r in rows {
        let failed: u64 = ["search", "get", "write"].iter().map(|k| r[k][1].as_u64().unwrap_or(0)).sum();
        if failed == 0 {
            clean += 1;
            start.get_or_insert(r["s"].as_u64().unwrap_or(0));
            if clean >= 10 {
                return start;
            }
        } else {
            clean = 0;
            start = None;
        }
    }
    None
}

pub async fn run(w: &World, names: &[String], dir: &Path) -> bool {
    let list: Vec<Scenario> = all().into_iter().filter(|s| names.is_empty() || names.iter().any(|n| n == s.name)).collect();
    let mut results = Vec::new();
    let mut all_ok = true;
    let (ok, why) = settle_start(w).await;
    w.events.add("note", &format!("cluster settled: {ok} {why}"), json!({}));
    for s in &list {
        let (settled, why) = settle(w, 150.0).await;
        if !settled {
            w.events.add("note", &format!("before {}: the cluster did not settle ({why}); going on", s.name), json!({}));
        }
        sleep(10.0).await;
        let base = look(w);
        let t0 = t(w);
        let sec0 = t0 as u64;
        w.events.add("scenario", &format!("== {}: {}", s.name, s.title), json!({"name": s.name}));
        let steps = play(w, s.name).await;
        let t_play = t(w);
        // Recovery: up to 3 minutes for the clients to see 10 s without a failure.
        let mut rec = None;
        let t1 = Instant::now();
        while t1.elapsed() < Duration::from_secs(180) {
            let now = w.events.start.elapsed().as_secs();
            rec = recovered_at(w, sec0, now);
            if rec.is_some() && healthy(w).0 {
                break;
            }
            sleep(1.0).await;
        }
        let end = t(w);
        let check = w.work.check().await;
        w.events.add("check", &format!("check: {}", crate::check_text(&check)), check.clone());
        let ok = check["ok"].as_bool() == Some(true);
        all_ok &= ok;
        let last_heal = steps.as_ref().ok().and_then(|s| s.as_array()?.iter().rev().find(|x| x["step"].as_str().is_some_and(|t| t.starts_with("heal") || t.starts_with("fix"))).and_then(|x| x["t"].as_f64()));
        let result = json!({
            "name": s.name, "title": s.title, "started_s": round1(t0), "played_s": round1(t_play - t0),
            "steps": steps.as_ref().cloned().unwrap_or_else(|e| json!([{"error": e}])),
            "error": steps.err(),
            "clients": w.work.stats.summary(sec0, t_play as u64 + 1),
            "clients_after": w.work.stats.summary(t_play as u64 + 1, end as u64 + 1),
            "recovered_s": rec.map(|r| r.saturating_sub(sec0)),
            "after_heal_recovered_s": rec.zip(last_heal).map(|(r, h)| (r as f64 - h).max(0.0).round()),
            "command_center": detection(w, t0, &base, end),
            "check": check,
            "settled_before": settled,
            "series": w.work.stats.series(sec0.saturating_sub(10), end as u64 + 1),
            "events": w.events.since(t0).into_iter().filter(|e| e["t"].as_f64().unwrap_or(0.0) <= end).map(|e| json!([e["t"], e["kind"], e["text"]])).collect::<Vec<_>>(),
        });
        results.push(result);
        write_results(dir, &results);
    }
    all_ok
}

fn write_results(dir: &Path, results: &[Json]) {
    let _ = std::fs::write(dir.join("results.json"), serde_json::to_string_pretty(&Json::Array(results.to_vec())).unwrap_or_default());
    let _ = std::fs::write(dir.join("results.md"), markdown(results));
}

fn markdown(results: &[Json]) -> String {
    let mut out = String::from("# strata-chaos results\n\n| scenario | searches failed | writes failed | writes failing for | first sign in the command center | alerts | recovered | counters |\n|---|---|---|---|---|---|---|---|\n");
    for r in results {
        let c = &r["clients"];
        let pct = |k: &str| match c[k]["calls"].as_u64() {
            Some(n) if n > 0 => format!("{} of {} ({:.1}%)", c[k]["failed"], n, 100.0 * c[k]["error_ratio"].as_f64().unwrap_or(0.0)),
            _ => "-".into(),
        };
        let sf = format!("{}; get {}", pct("search"), pct("get"));
        let span = match (c["write"]["errors_from_s"].as_u64(), c["write"]["errors_to_s"].as_u64()) {
            (Some(a), Some(b)) => format!("{} s ({} s with errors)", b - a, c["write"]["seconds_with_errors"]),
            _ => "-".into(),
        };
        let cc = &r["command_center"];
        let first = match cc["first_signal_s"].as_f64() {
            Some(s) => format!("{s} s: {}", cc["first_signal"].as_str().unwrap_or("")),
            None => "nothing".into(),
        };
        let alerts: Vec<String> = cc["alerts"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)).collect();
        let check = if r["check"]["ok"].as_bool() == Some(true) { "ok".to_string() } else { format!("**{}**", crate::check_text(&r["check"])) };
        let rec = r["recovered_s"].as_u64().map_or("not within 3 min".into(), |s| format!("{s} s"));
        out += &format!("| {} | {} | {} | {} | {} | {} | {} | {} |\n", r["name"].as_str().unwrap_or(""), sf, pct("write"), span, first.replace('|', "/"), if alerts.is_empty() { "-".into() } else { alerts.join("; ").replace('|', "/") }, rec, check);
    }
    out += "\nErrors by class, per scenario:\n\n";
    for r in results {
        let c = &r["clients"];
        let mut parts = Vec::new();
        for k in ["search", "get", "write", "flood", "hot"] {
            if let Some(e) = c[k]["errors"].as_object() {
                if !e.is_empty() {
                    parts.push(format!("{k}: {}", e.iter().map(|(a, b)| format!("{a} x{b}")).collect::<Vec<_>>().join(", ")));
                }
            }
        }
        out += &format!("- **{}**: {}; p99 search {} ms, write {} ms\n", r["name"].as_str().unwrap_or(""), if parts.is_empty() { "none".into() } else { parts.join("; ") }, c["search"]["p99_ms"], c["write"]["p99_ms"]);
    }
    out
}

/// Random faults for `secs` seconds: one at a time on at most one copy
/// per shard, each held for a while and healed, then the check.
pub async fn monkey(w: &World, secs: u64, seed: u64, gap_s: u64, dir: &Path) -> bool {
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
    let (ok, why) = settle_start(w).await;
    w.events.add("note", &format!("monkey: {secs} s of random faults, seed {seed}; cluster settled: {ok} {why}"), json!({}));
    let t_end = Instant::now() + Duration::from_secs(secs);
    let mut checks = Vec::new();
    let mut n = 0;
    while Instant::now() < t_end {
        sleep(rng.gen_range(gap_s / 2..=gap_s * 3 / 2).max(1) as f64).await;
        let shard = rng.gen_range(0..w.cluster.layout.shards);
        let role = if rng.gen_bool(0.5) { format!("primary:{shard}") } else { format!("replica:{shard}") };
        let hold = rng.gen_range(3.0..40.0);
        let r = match rng.gen_range(0..8) {
            0 => Inject { fault: "kill".into(), target: Some(role), ..Inject::default() },
            1 => Inject { fault: "stop".into(), target: Some(role), ..Inject::default() },
            2 => Inject { fault: "pause".into(), target: Some(role), ..Inject::default() },
            3 => Inject { fault: "slow-cpu".into(), target: Some(role), pct: Some(rng.gen_range(50..95)), ..Inject::default() },
            4 => Inject { fault: "delay".into(), target: Some(role), ms: Some(rng.gen_range(20..800)), ..Inject::default() },
            5 => Inject { fault: "isolate".into(), target: Some(role), ..Inject::default() },
            6 => Inject { fault: "cut".into(), target: Some(role), to: Some(if rng.gen_bool(0.5) { "coord".into() } else { format!("replica:{shard}") }), oneway: rng.gen_bool(0.5), ..Inject::default() },
            _ => Inject { fault: "kill".into(), target: Some("coord".into()), ..Inject::default() },
        };
        match w.monkey.inject(&r) {
            Ok(j) => {
                n += 1;
                sleep(hold).await;
                if j["active"].as_bool() == Some(true) {
                    let _ = w.monkey.heal(j["id"].as_str().unwrap_or(""));
                }
            }
            Err(e) => w.events.add("note", &format!("monkey: {} {:?} not injected: {e}", r.fault, r.target), json!({})),
        }
        // Every tenth fault, and at the end: the counters.
        if n % 10 == 0 {
            let _ = settle(w, 60.0).await;
            let c = w.work.check().await;
            w.events.add("check", &format!("check: {}", crate::check_text(&c)), c.clone());
            checks.push(c);
        }
    }
    w.monkey.heal_all();
    let (settled, why) = settle(w, 180.0).await;
    let c = w.work.check().await;
    w.events.add("check", &format!("check after {n} faults: {}", crate::check_text(&c)), c.clone());
    checks.push(c);
    let ok = checks.iter().all(|c| c["ok"].as_bool() == Some(true));
    let end = w.events.start.elapsed().as_secs();
    let result = json!({"faults": n, "seed": seed, "seconds": secs, "settled": settled, "settle_note": why, "checks": checks, "clients": w.work.stats.summary(0, end + 1), "events": w.events.all()});
    let _ = std::fs::write(dir.join("monkey.json"), serde_json::to_string_pretty(&result).unwrap_or_default());
    let _ = w.work.stop.load(Ordering::Relaxed);
    ok
}
