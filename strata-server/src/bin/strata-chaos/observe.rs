//! What happened, in order (`events.jsonl`), and what the cluster said of
//! itself meanwhile: each second the observer asks a coordinator for its
//! fleet health (Home's headline and problems), the alerts firing and the
//! cluster map, and records each change, so that a run can say when the
//! command center first showed a fault and when it cleared.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value as Json};

use crate::cluster::Cluster;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// The run's log of what happened.
pub struct Events {
    pub start: Instant,
    pub start_ms: u64,
    list: Mutex<Vec<Json>>,
    file: Mutex<Option<std::fs::File>>,
    pub quiet: bool,
}

impl Events {
    pub fn new(path: &Path, quiet: bool) -> Events {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
        Events { start: Instant::now(), start_ms: now_ms(), list: Mutex::default(), file: Mutex::new(file), quiet }
    }

    pub fn t(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Records an event: its kind (`fault`, `heal`, `process`, `fleet`,
    /// `problem`, `alert`, `map`, `workload`, `note`, `check`), a sentence
    /// and its details.
    pub fn add(&self, kind: &str, text: &str, data: Json) {
        let e = json!({"t": (self.t() * 10.0).round() / 10.0, "at_ms": now_ms(), "kind": kind, "text": text, "data": data});
        if !self.quiet {
            eprintln!("[{:>7.1}s] {kind:<8} {text}", self.t());
        }
        if let Some(f) = self.file.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{e}");
        }
        self.list.lock().unwrap().push(e);
    }

    pub fn since(&self, t: f64) -> Vec<Json> {
        self.list.lock().unwrap().iter().filter(|e| e["t"].as_f64().unwrap_or(0.0) >= t).cloned().collect()
    }

    pub fn all(&self) -> Vec<Json> {
        self.list.lock().unwrap().clone()
    }
}

/// What the cluster said of itself at one look.
#[derive(Clone, Debug, Default)]
pub struct Look {
    pub t: f64,
    /// Home's status (`ok`, `warning`, `critical`) and headline.
    pub status: String,
    pub headline: String,
    /// Problem id → its title.
    pub problems: BTreeMap<String, String>,
    /// Alert rule → "name (severity) on where".
    pub alerts: BTreeMap<String, String>,
    /// Each shard's servers by name, the primary first.
    pub shards: Vec<Vec<String>>,
    pub primaries: Vec<Option<String>>,
    pub epoch: u64,
    pub error: Option<String>,
    pub coordinator: String,
}

/// The observer's latest look, and the looks of the run (one a second).
#[derive(Default)]
pub struct Seen {
    pub last: Mutex<Look>,
    pub looks: Mutex<Vec<Look>>,
    /// Each shard's slots, as `[start, end)` ranges.
    pub slot_ranges: Mutex<Vec<Vec<(usize, usize)>>>,
}

pub async fn observe(cluster: Arc<Cluster>, seen: Arc<Seen>, http: reqwest::Client, stop: Arc<std::sync::atomic::AtomicBool>) {
    let mut prev = Look::default();
    let auth = format!("Bearer {}", cluster.layout.admin_token);
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let tick = Instant::now();
        let mut look = Look { t: cluster.events.t(), ..Look::default() };
        let mut answered = false;
        for c in cluster.coordinators() {
            match fetch(&http, &auth, &c).await {
                Ok((fleet, alerts, map)) => {
                    look.coordinator = cluster.name_of_url(&c).unwrap_or(c.clone());
                    read_fleet(&fleet, &mut look);
                    read_alerts(&alerts, &mut look);
                    read_map(&cluster, &map, &mut look);
                    let ranges: Vec<Vec<(usize, usize)>> = map["shards"].as_array().into_iter().flatten().map(|s| s["slots"].as_array().into_iter().flatten().filter_map(|r| Some((r[0].as_u64()? as usize, r[1].as_u64()? as usize))).collect()).collect();
                    if !ranges.is_empty() {
                        *seen.slot_ranges.lock().unwrap() = ranges;
                    }
                    answered = true;
                    break;
                }
                Err(e) => look.error = Some(format!("{}: {e}", cluster.name_of_url(&c).unwrap_or(c.clone()))),
            }
        }
        if !answered {
            look.status = "unreachable".into();
            look.headline = "no coordinator answers".into();
        } else {
            look.error = None;
        }
        record_changes(&cluster, &prev, &look);
        if answered && !look.shards.is_empty() {
            let urls: Vec<Vec<String>> = look.shards.iter().map(|s| s.iter().filter_map(|n| cluster.with(n, |p| p.grpc_url()).ok()).collect()).collect();
            if urls.iter().all(|s| !s.is_empty()) {
                *cluster.map.lock().unwrap() = urls;
            }
        }
        *seen.last.lock().unwrap() = look.clone();
        seen.looks.lock().unwrap().push(look.clone());
        prev = look;
        let spent = tick.elapsed();
        if spent < Duration::from_secs(1) {
            tokio::time::sleep(Duration::from_secs(1) - spent).await;
        }
    }
}

async fn fetch(http: &reqwest::Client, auth: &str, base: &str) -> Result<(Json, Json, Json), String> {
    let get = |path: &str| {
        let url = format!("{base}{path}");
        let req = http.get(url).header("authorization", auth).header("x-strata-client", "strata-chaos").timeout(Duration::from_secs(3));
        async move {
            let r = req.send().await.map_err(|e| e.to_string())?;
            let st = r.status();
            let j: Json = r.json().await.map_err(|e| e.to_string())?;
            if !st.is_success() && j.get("status").is_none() {
                return Err(format!("{st}: {}", j["error"].as_str().unwrap_or("")));
            }
            Ok::<Json, String>(j)
        }
    };
    let (f, a, m) = tokio::join!(get("/v1/health/fleet"), get("/v1/alerts"), get("/v1/cluster"));
    Ok((f?, a.unwrap_or(Json::Null), m.unwrap_or(Json::Null)))
}

fn read_fleet(f: &Json, look: &mut Look) {
    look.status = f["status"].as_str().unwrap_or("?").to_string();
    look.headline = f["headline"].as_str().map(str::to_string).or_else(|| f["headline"]["title"].as_str().map(str::to_string)).unwrap_or_default();
    for p in f["problems"].as_array().into_iter().flatten() {
        let id = p["id"].as_str().unwrap_or("?").to_string();
        look.problems.insert(id, p["title"].as_str().unwrap_or("").to_string());
    }
}

fn read_alerts(a: &Json, look: &mut Look) {
    for x in a["states"].as_array().into_iter().flatten() {
        if x["state"].as_str() != Some("firing") {
            continue;
        }
        let rule = x["rule"].as_str().unwrap_or("?").to_string();
        let wh: Vec<String> = x["where"].as_array().into_iter().flatten().filter_map(|w| w["short"].as_str().or_else(|| w["name"].as_str()).or_else(|| w.as_str()).map(str::to_string)).collect();
        let text = format!("{} ({}){}", x["name"].as_str().unwrap_or(&rule), x["severity"].as_str().unwrap_or("?"), if wh.is_empty() { String::new() } else { format!(" on {}", wh.join(", ")) });
        look.alerts.insert(rule, text);
    }
}

fn read_map(cluster: &Cluster, m: &Json, look: &mut Look) {
    look.epoch = m["slot_epoch"].as_u64().unwrap_or(0);
    for s in m["shards"].as_array().into_iter().flatten() {
        let mut names = Vec::new();
        let mut primary = None;
        for r in s["replicas"].as_array().into_iter().flatten() {
            let Some(n) = r["url"].as_str().and_then(|u| cluster.name_of_url(u)) else { continue };
            names.push(n);
        }
        if let Some(n) = s["primary"].as_str().and_then(|u| cluster.name_of_url(u)) {
            primary = Some(n);
        }
        if let Some(p) = &primary {
            names.retain(|n| n != p);
            names.insert(0, p.clone());
        }
        look.shards.push(names);
        look.primaries.push(primary);
    }
}

fn record_changes(cluster: &Cluster, a: &Look, b: &Look) {
    let ev = &cluster.events;
    if a.status != b.status || a.headline != b.headline {
        ev.add("fleet", &format!("Home: {} — {}", b.status, if b.headline.is_empty() { "all right" } else { &b.headline }), json!({"status": b.status, "headline": b.headline}));
    }
    let (pa, pb): (BTreeSet<_>, BTreeSet<_>) = (a.problems.keys().collect(), b.problems.keys().collect());
    for id in pb.difference(&pa) {
        ev.add("problem", &format!("problem {id}: {}", b.problems[*id]), json!({"id": id, "title": b.problems[*id], "state": "on"}));
    }
    for id in pa.difference(&pb) {
        ev.add("problem", &format!("problem {id} cleared"), json!({"id": id, "state": "off"}));
    }
    let (aa, ab): (BTreeSet<_>, BTreeSet<_>) = (a.alerts.keys().collect(), b.alerts.keys().collect());
    for r in ab.difference(&aa) {
        ev.add("alert", &format!("alert firing: {}", b.alerts[*r]), json!({"rule": r, "text": b.alerts[*r], "state": "firing"}));
    }
    for r in aa.difference(&ab) {
        ev.add("alert", &format!("alert resolved: {r}"), json!({"rule": r, "state": "resolved"}));
    }
    if !b.shards.is_empty() && (a.primaries != b.primaries || a.shards != b.shards) {
        let text: Vec<String> = b.shards.iter().enumerate().map(|(i, s)| format!("shard {i}: {}", s.join(" "))).collect();
        ev.add("map", &format!("map (epoch {}): {} — primaries first", b.epoch, text.join("; ")), json!({"epoch": b.epoch, "shards": b.shards, "primaries": b.primaries}));
    }
}
