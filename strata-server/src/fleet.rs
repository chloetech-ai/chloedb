//! Fleet health (`GET /v1/health/fleet`): whether the cluster is all
//! right and, when it is not, what is wrong first, in one answer the
//! command center's Home shows as its headline.
//!
//! It judges what the coordinator already holds: the shared cluster
//! snapshot (each shard's state, each copy's detail) and its own points
//! over the last five minutes (`WINDOW_S`), which describe the clients'
//! searches. Each check says `ok`, `warning` or `critical`; each problem
//! says where it is, since when, what it does to clients and what to do
//! next, worst first. On a shard server the same checks describe that
//! server alone.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value as Json};

/// The window of the SLO checks, in seconds.
pub const WINDOW_S: u64 = 300;
/// How long a copy's disk use is remembered to tell how fast it grows.
const DISK_MEMORY_MS: u64 = 3_600_000;
/// The shortest span of disk use that a growth rate is read from.
const DISK_SPAN_MS: u64 = 600_000;

/// What fleet health remembers between answers: since when its status
/// and each problem hold, and each copy's disk use over the last hour.
#[derive(Default)]
pub struct Fleet {
    memory: Mutex<Memory>,
}

#[derive(Default)]
struct Memory {
    status: Option<(&'static str, u64)>,
    problems: HashMap<String, u64>,
    disk: HashMap<String, VecDeque<(u64, f64)>>,
}

/// What a judgement reads.
pub struct Inputs<'a> {
    /// The cluster: `{"shards": [...]}` as `/v1/cluster` has it, each shard
    /// with its `state`, `why` and copies.
    pub cluster: &'a Json,
    /// This server's points over the window.
    pub points: &'a [Arc<Json>],
    pub slo_error_ratio: f64,
    pub slo_p99_ms: f64,
    /// Alerts this server fires: all of them, and the critical ones.
    pub alerts: (u64, u64),
    /// Whether searches leave out a shard that does not answer.
    pub partial: bool,
    /// This server, when it is a coordinator: `{"url", "name"}`.
    pub coordinator: Option<Json>,
    pub now_ms: u64,
}

/// A problem found, with what ranks it.
struct Problem {
    rank: u32,
    critical: bool,
    headline: String,
    j: Json,
}

fn severity(critical: bool) -> &'static str {
    if critical {
        "critical"
    } else {
        "warning"
    }
}

fn worst(a: &'static str, b: &str) -> &'static str {
    match (a, b) {
        ("critical", _) | (_, "critical") => "critical",
        ("warning", _) | (_, "warning") => "warning",
        _ => "ok",
    }
}

/// `a`, `a or b`, `a, b or c`.
fn either(v: &[String]) -> String {
    match v {
        [] => String::new(),
        [a] => a.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// `a`, `a and b`, `a, b and c`.
fn both(v: &[String]) -> String {
    match v {
        [] => String::new(),
        [a] => a.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn pct(r: f64) -> String {
    let p = r * 100.0;
    if p == 0.0 {
        "0%".into()
    } else if p < 0.1 {
        format!("{p:.2}%")
    } else if p < 10.0 {
        format!("{p:.1}%").replace(".0%", "%")
    } else {
        format!("{p:.0}%")
    }
}

/// The searches of the window: how many ended, how many failed on the
/// server's side, the latency histogram, and the mean rate and CPU.
#[derive(Default)]
pub struct Window {
    pub queries: f64,
    pub failures: f64,
    pub hist: Vec<u64>,
    pub secs: f64,
    pub cpu_s: f64,
    /// Points, oldest first, with their failure ratio (`None` without
    /// searches).
    ratios: Vec<(u64, Option<f64>)>,
}

impl Window {
    pub fn of(points: &[Arc<Json>]) -> Window {
        let mut w = Window::default();
        for p in points {
            let secs = p["secs"].as_f64().unwrap_or(2.0);
            // Counts where the point has them (server-side failures only);
            // else from the rates.
            let n = p["queries"].as_f64().unwrap_or_else(|| p["qps"].as_f64().unwrap_or(0.0) * secs);
            let f = p["query_errors"].as_f64().unwrap_or_else(|| p["errors_s"].as_f64().unwrap_or(0.0) * secs);
            w.queries += n;
            w.failures += f;
            w.secs += secs;
            w.cpu_s += p["cpu"].as_f64().unwrap_or(0.0) * secs;
            if let Some(h) = p["latency_hist"].as_array() {
                w.hist.resize(w.hist.len().max(h.len()), 0);
                for (i, c) in h.iter().enumerate() {
                    w.hist[i] += c.as_u64().unwrap_or(0);
                }
            }
            w.ratios.push((p["t"].as_u64().unwrap_or(0), (n > 0.0).then(|| f / n)));
        }
        w
    }

    pub fn error_ratio(&self) -> Option<f64> {
        (self.queries > 0.0).then(|| self.failures / self.queries)
    }

    pub fn qps(&self) -> Option<f64> {
        (self.secs > 0.0).then(|| self.queries / self.secs)
    }

    pub fn cpu(&self) -> Option<f64> {
        (self.secs > 0.0).then(|| self.cpu_s / self.secs)
    }

    pub fn p99_ms(&self) -> Option<f64> {
        if self.hist.iter().sum::<u64>() == 0 {
            return None;
        }
        crate::telemetry::quantile(&crate::metrics::BUCKETS, &self.hist, 0.99).map(|s| s * 1e3)
    }

    /// Since when the points' failure ratio stays over `slo`: the first
    /// point of the latest run of them.
    fn over_since(&self, slo: f64) -> Option<u64> {
        let mut since = None;
        for &(t, r) in &self.ratios {
            match r {
                Some(r) if r > slo => {
                    since.get_or_insert(t);
                }
                Some(_) => since = None,
                None => {}
            }
        }
        since
    }
}

/// A copy's short name (`s1-0`), else its url.
fn short(r: &Json) -> String {
    r["short"].as_str().or(r["url"].as_str()).unwrap_or("?").to_string()
}

fn down(r: &Json) -> bool {
    r["status"] == "down"
}

impl Fleet {
    /// Fleet health of `i` (see the module's doc).
    pub fn judge(&self, i: Inputs) -> Json {
        let mut m = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        let now = i.now_ms;
        let shards: Vec<Json> = i.cluster["shards"].as_array().cloned().unwrap_or_default();
        let mut checks = Vec::new();
        let mut problems: Vec<Problem> = Vec::new();

        // ----- shards
        let (mut no_reads, mut no_writes, mut degraded, mut downs) = (0, 0, 0, 0);
        let mut where_shards = Vec::new();
        for s in &shards {
            let n = s["shard"].as_u64().unwrap_or(0);
            let state = s["state"].as_str().unwrap_or("ok");
            let copies = s["replicas"].as_array().cloned().unwrap_or_default();
            let gone: Vec<String> = copies.iter().filter(|r| down(r)).map(short).collect();
            let up: Vec<String> = copies.iter().filter(|r| !down(r)).map(short).collect();
            let since = copies.iter().filter_map(|r| r["down_since_ms"].as_u64()).min();
            let why = s["why"].as_str().unwrap_or("").to_string();
            let name = format!("shard {n}");
            let mut where_: Vec<String> = vec![name.clone()];
            where_.extend(gone.iter().cloned());
            let next = |text: String| json!({"text": text, "view": "cluster", "params": {"shard": n}});
            let searches_fail = if i.partial { format!("Searches answer without {name}'s rows") } else { format!("Every search fails: each one needs {name}") };
            let p = match state {
                "ok" => None,
                "down" => {
                    downs += 1;
                    Some((0, true, format!("Searches are failing: {name} is down"), format!("Shard {n} is down: none of its {} copies answers", copies.len()), format!("{searches_fail}, and writes to its rows fail"), next(format!("Bring back {}", either(&gone)))))
                }
                "no_reads" => {
                    no_reads += 1;
                    let majority_lost = s["failover"] == true && s["majority"] == false;
                    let (head, title, todo) = if majority_lost {
                        let them = if gone.len() == 1 { "it" } else { "them" };
                        let has = if up.len() == 1 { "has" } else { "have" };
                        (
                            format!("Searches are failing: {name} has no majority"),
                            format!("Shard {n} has no majority, so it serves no reads"),
                            format!("Bring {} back, or remove {them} so {} {has} a majority", either(&gone), both(&up)),
                        )
                    } else {
                        (format!("Searches are failing: {name} serves no reads"), format!("Shard {n} serves no reads"), format!("Find out why on its copies: {why}"))
                    };
                    Some((1, true, head, title, searches_fail, next(todo)))
                }
                "no_writes" => {
                    no_writes += 1;
                    let elects = s["majority"] != false;
                    let todo = if s["primary"].is_null() && s["failover"] == true && !elects {
                        // No majority answers: no election can win.
                        let mut t = crate::coord::elect_by_hand(&gone, &up);
                        t[..1].make_ascii_uppercase();
                        t
                    } else if s["primary"].is_null() && s["failover"] == true {
                        "An election should choose a primary within seconds. If none does, bring a copy back or hand the primary over by hand".to_string()
                    } else if s["primary"].is_null() {
                        "Hand the primary over to an up-to-date copy".to_string()
                    } else {
                        format!("Find out why on its primary: {why}")
                    };
                    let head = if s["primary"].is_null() { format!("Writes are failing: {name} has no primary") } else { format!("Writes are failing: {name} takes no writes") };
                    let title = if s["primary"].is_null() && s["failover"] == true && !elects {
                        // "Shard 0 cannot elect a primary or take writes until a second copy answers"
                        let c = crate::coord::cannot_elect(copies.len(), up.len());
                        format!("Shard {n} {}", c.trim_start_matches("it ").replacen("elect a primary", "elect a primary or take writes", 1))
                    } else {
                        format!("Shard {n} takes no writes")
                    };
                    Some((2, true, head, title, format!("Writes to {name}'s rows fail; searches still answer"), next(todo)))
                }
                _ => {
                    degraded += 1;
                    let more = if why.contains("one more copy down") { "; one more copy down stops its reads and writes" } else { "" };
                    let them = if gone.len() == 1 { "it" } else { "them" };
                    Some((
                        10,
                        false,
                        format!("{} of {name}'s {} copies {} down; it still serves reads and takes writes", gone.len(), copies.len(), if gone.len() == 1 { "is" } else { "are" }),
                        format!("Shard {n} has {} of {} copies down", gone.len(), copies.len()),
                        format!("None yet: the shard serves reads and takes writes{more}"),
                        next(format!("Bring {} back, or replace {them}", either(&gone))),
                    ))
                }
            };
            if let Some((rank, critical, headline, title, impact, next)) = p {
                where_shards.push(name.clone());
                problems.push(Problem {
                    rank,
                    critical,
                    headline,
                    j: json!({"id": format!("shard-{n}-{state}"), "severity": severity(critical), "title": title, "where": where_, "since_ms": since, "impact": impact, "next": next, "why": why}),
                });
            }
        }
        let shards_status = if downs + no_reads + no_writes > 0 {
            "critical"
        } else if degraded > 0 {
            "warning"
        } else {
            "ok"
        };
        checks.push(json!({"id": "shards", "status": shards_status, "value": {"down": downs, "no_reads": no_reads, "no_writes": no_writes, "degraded": degraded, "total": shards.len()}, "where": where_shards}));

        // ----- the clients' searches over the window
        let w = Window::of(i.points);
        let ratio = w.error_ratio();
        let slo = i.slo_error_ratio;
        let er_status = match ratio {
            Some(r) if r > slo => "critical",
            Some(r) if r > slo / 2.0 => "warning",
            _ => "ok",
        };
        let er_since = (er_status != "ok").then(|| w.over_since(slo / 2.0)).flatten();
        let mut er = json!({"id": "error_ratio", "status": er_status, "value": ratio, "slo": slo, "window_s": WINDOW_S, "queries": w.queries.round(), "failures": w.failures.round()});
        if let Some(s) = er_since {
            er["since_ms"] = json!(s);
        }
        if ratio.is_none() {
            er["note"] = json!("No search ended in the last 5 min");
        }
        checks.push(er);
        if let (Some(r), true) = (ratio, er_status != "ok") {
            let critical = er_status == "critical";
            problems.push(Problem {
                rank: if critical { 3 } else { 12 },
                critical,
                headline: format!("Searches are failing: {} failed over the last 5 min (SLO {})", pct(r), pct(slo)),
                j: json!({"id": "error_ratio", "severity": severity(critical), "title": format!("{} of searches failed over the last 5 min (SLO {})", pct(r), pct(slo)), "where": ["searches"], "since_ms": er_since,
                    "impact": format!("{} of searches fail", pct(r)), "next": {"text": "See which statements fail, and on which shards", "view": "queries", "params": {"outcome": "error"}}}),
            });
        }
        let p99 = w.p99_ms();
        let slo_ms = i.slo_p99_ms;
        let p99_status = match p99 {
            Some(v) if v > slo_ms * 4.0 => "critical",
            Some(v) if v > slo_ms => "warning",
            _ => "ok",
        };
        checks.push(json!({"id": "p99", "status": p99_status, "value_ms": p99, "slo_ms": slo_ms, "window_s": WINDOW_S}));
        if let (Some(v), true) = (p99, p99_status != "ok") {
            let critical = p99_status == "critical";
            problems.push(Problem {
                rank: if critical { 4 } else { 13 },
                critical,
                headline: format!("Searches are slow: p99 {v:.0} ms over the last 5 min (SLO {slo_ms:.0} ms)"),
                j: json!({"id": "p99", "severity": severity(critical), "title": format!("Searches are slow: p99 {v:.0} ms over the last 5 min (SLO {slo_ms:.0} ms)"), "where": ["searches"], "since_ms": null,
                    "impact": format!("1 search in 100 takes over {v:.0} ms"), "next": {"text": "See the slowest statements and where their time goes", "view": "performance", "params": {}}}),
            });
        }

        // ----- copies
        let copies: Vec<(usize, Json)> = shards.iter().enumerate().flat_map(|(k, s)| s["replicas"].as_array().cloned().unwrap_or_default().into_iter().map(move |r| (k, r))).collect();
        let gone: Vec<String> = copies.iter().filter(|(_, r)| down(r)).map(|(_, r)| short(r)).collect();
        checks.push(json!({"id": "copies", "status": if gone.is_empty() { "ok" } else { "warning" }, "down": gone.len(), "total": copies.len(), "where": gone}));

        // ----- lag: what each primary says its replicas lack
        let mut lag: Option<(u64, Option<f64>, String, usize, bool)> = None;
        let mut out_of_sync = Vec::new();
        for (k, s) in shards.iter().enumerate() {
            let rs = s["replicas"].as_array().cloned().unwrap_or_default();
            let Some(p) = rs.iter().find(|r| !down(r) && r["detail"]["health"]["replication"]["role"] == "primary") else { continue };
            let writes_s = p["detail"]["load"]["writes_s"].as_f64().filter(|w| *w > 0.0);
            for f in p["detail"]["health"]["replication"]["replicas"].as_array().cloned().unwrap_or_default() {
                let copy = rs.iter().find(|r| r["url"] == f["url"]);
                if copy.is_some_and(down) {
                    continue;
                }
                let name = copy.map_or_else(|| f["url"].as_str().unwrap_or("?").to_string(), short);
                let behind = f["behind"].as_u64().unwrap_or(0);
                if f["in_sync"] == false && f["member"] != false {
                    out_of_sync.push(name.clone());
                }
                if lag.as_ref().is_none_or(|l| behind > l.0) {
                    lag = Some((behind, writes_s.map(|w| behind as f64 / w), name, s["shard"].as_u64().unwrap_or(k as u64) as usize, f["in_sync"] != false));
                }
            }
        }
        let lag_status = match &lag {
            Some((_, Some(secs), ..)) if *secs >= 30.0 => "warning",
            _ if !out_of_sync.is_empty() => "warning",
            _ => "ok",
        };
        let mut lc = json!({"id": "lag", "status": lag_status, "max_txn": lag.as_ref().map_or(0, |l| l.0), "max_s": lag.as_ref().and_then(|l| l.1).map(|s| (s * 10.0).round() / 10.0), "where": lag.as_ref().map(|l| l.2.clone()), "out_of_sync": out_of_sync.clone()});
        if lag.as_ref().is_some_and(|l| l.1.is_some()) {
            lc["max_s_from"] = json!("transactions behind over the primary's write rate");
        }
        checks.push(lc);
        if let (Some((txn, secs, name, shard, _)), "warning") = (&lag, lag_status) {
            let about = secs.map_or(String::new(), |s| format!(" (about {s:.0} s)"));
            let title = if out_of_sync.is_empty() { format!("{name} is {txn} transactions behind its primary{about}") } else { format!("{} not in sync with {} primary", both(&out_of_sync), if out_of_sync.len() == 1 { "its" } else { "their" }) };
            problems.push(Problem {
                rank: 14,
                critical: false,
                headline: title.clone(),
                j: json!({"id": "lag", "severity": "warning", "title": title, "where": [format!("shard {shard}"), name], "since_ms": null,
                    "impact": format!("Searches {name} serves may miss recent writes; it may not count toward a write majority"), "next": {"text": format!("Look at {name}'s replication"), "view": "cluster", "params": {"shard": shard, "copy": name}}}),
            });
        }

        // ----- disk
        let mut disks: Vec<(f64, String, usize)> = Vec::new();
        for (k, r) in copies.iter().filter(|(_, r)| !down(r)) {
            let d = &r["detail"];
            if let Some(v) = d["load"]["disk_used_ratio"].as_f64().or(d["minute"]["disk_used_ratio"].as_f64()) {
                let name = short(r);
                let seen = m.disk.entry(name.clone()).or_default();
                if seen.back().is_none_or(|&(t, _)| now >= t + 10_000) {
                    seen.push_back((now, v));
                }
                while seen.front().is_some_and(|&(t, _)| t + DISK_MEMORY_MS < now) {
                    seen.pop_front();
                }
                disks.push((v, name, shards[*k]["shard"].as_u64().unwrap_or(*k as u64) as usize));
            }
        }
        let names: std::collections::HashSet<&String> = disks.iter().map(|d| &d.1).collect();
        m.disk.retain(|k, _| names.contains(k));
        let mut dc = json!({"id": "disk", "status": "ok"});
        if let Some((max, name, shard)) = disks.iter().cloned().max_by(|a, b| a.0.total_cmp(&b.0)) {
            let mut sorted: Vec<f64> = disks.iter().map(|d| d.0).collect();
            sorted.sort_by(f64::total_cmp);
            let median = sorted[sorted.len() / 2];
            // Days until the fullest disk fills, at the rate it grew over the
            // span remembered (ten minutes at least).
            let days = m.disk.get(&name).and_then(|s| {
                let (&(t0, v0), &(t1, v1)) = (s.front()?, s.back()?);
                let rate = (v1 - v0) / (t1.saturating_sub(t0)) as f64;
                (t1 >= t0 + DISK_SPAN_MS && rate > 0.0).then(|| (1.0 - v1) / rate / 86_400_000.0)
            });
            let status = if max >= 0.95 {
                "critical"
            } else if max >= 0.85 || days.is_some_and(|d| d < 3.0) {
                "warning"
            } else {
                "ok"
            };
            let r2 = |v: f64| (v * 1000.0).round() / 1000.0;
            dc = json!({"id": "disk", "status": status, "max": r2(max), "median": r2(median), "where": name, "days_to_full": days.map(|d| (d * 10.0).round() / 10.0)});
            if status != "ok" {
                let critical = status == "critical";
                let fills = days.map_or(String::new(), |d| format!(" and fills in about {d:.1} days at the last hour's rate"));
                problems.push(Problem {
                    rank: if critical { 5 } else { 15 },
                    critical,
                    headline: format!("{name}'s disk is {} full{fills}", pct(max)),
                    j: json!({"id": format!("disk-{name}"), "severity": severity(critical), "title": format!("{name}'s disk is {} full{fills}", pct(max)), "where": [format!("shard {shard}"), name], "since_ms": null,
                        "impact": format!("Writes to shard {shard} stop when its disk is full"), "next": {"text": "See which tables grow, then free space, add disk or add a shard", "view": "tables", "params": {"shard": shard}}}),
                });
            }
        }
        checks.push(dc);

        // ----- alerts: this server's, and each copy's own
        let (mut firing, mut critical) = i.alerts;
        for (_, r) in &copies {
            for a in r["detail"]["alerts"]["firing"].as_array().map_or(&[][..], |v| v.as_slice()) {
                firing += 1;
                critical += u64::from(a["severity"] == "critical");
            }
        }
        let al_status = if critical > 0 {
            "critical"
        } else if firing > 0 {
            "warning"
        } else {
            "ok"
        };
        checks.push(json!({"id": "alerts", "status": al_status, "firing": firing, "critical": critical}));
        if firing > 0 {
            let title = format!("{firing} {} firing{}", if firing == 1 { "alert is" } else { "alerts are" }, if critical > 0 { format!(", {critical} critical") } else { String::new() });
            problems.push(Problem {
                rank: if critical > 0 { 6 } else { 16 },
                critical: critical > 0,
                headline: title.clone(),
                j: json!({"id": "alerts", "severity": severity(critical > 0), "title": title, "where": [], "since_ms": null, "impact": "See each alert for what it measures", "next": {"text": "Open the alerts firing", "view": "alerts", "params": {}}}),
            });
        }

        // ----- since when each problem holds, worst first
        let ids: std::collections::HashSet<String> = problems.iter().map(|p| p.j["id"].as_str().unwrap_or("").to_string()).collect();
        m.problems.retain(|k, _| ids.contains(k));
        for p in &mut problems {
            let id = p.j["id"].as_str().unwrap_or("").to_string();
            let first = *m.problems.entry(id).or_insert(now);
            if p.j["since_ms"].is_null() {
                p.j["since_ms"] = json!(first);
            }
        }
        problems.sort_by_key(|p| (!p.critical, p.rank, p.j["since_ms"].as_u64().unwrap_or(now)));

        let status = checks.iter().fold("ok", |a, c| worst(a, c["status"].as_str().unwrap_or("ok")));
        let since = match m.status {
            Some((s, t)) if s == status => t,
            _ => now,
        };
        m.status = Some((status, since));
        let headline = match problems.first() {
            Some(p) => p.headline.clone(),
            None => {
                let copies_up = copies.len() - gone.len();
                let all = if shards.len() == 1 { "Its 1 shard serves reads and takes writes".to_string() } else { format!("All {} shards serve reads and take writes", shards.len()) };
                let mut h = format!("{all}; {copies_up} of {} {} up", copies.len(), if copies.len() == 1 { "copy is" } else { "copies are" });
                if shards.len() == 1 && i.coordinator.is_none() {
                    h = "This server serves reads and takes writes".into();
                }
                match (ratio, p99) {
                    (Some(r), Some(v)) => format!("{h}. Over the last 5 min {} of searches failed and p99 was {v:.0} ms", pct(r)),
                    _ => format!("{h}. No search ended in the last 5 min"),
                }
            }
        };
        let since_ms = problems.first().and_then(|p| p.j["since_ms"].as_u64()).map_or(since, |t| t.min(since));
        let coordinators: Vec<Json> = i
            .coordinator
            .iter()
            .map(|c| {
                json!({"url": c["url"], "name": c["name"], "qps": w.qps(), "p99_ms": p99, "error_ratio": ratio, "cpu": w.cpu(), "this": true})
            })
            .collect();
        json!({
            "at_ms": now, "status": status, "headline": headline, "since_ms": since_ms,
            "api": {"ok": true, "server": if i.coordinator.is_some() { "coordinator" } else { "shard" }},
            "checks": checks,
            "problems": problems.into_iter().map(|p| p.j).collect::<Vec<_>>(),
            "coordinators": coordinators,
            "window_s": WINDOW_S,
        })
    }
}

/// A shard server as one shard of itself, for fleet health on a server
/// that is not a coordinator: its state from what it serves (`detail`'s
/// `capability`), its copies itself and the replicas it ships to.
pub fn local_shard(detail: &Json) -> Json {
    let cap = &detail["capability"];
    let rep = &detail["health"]["replication"];
    let role = rep["role"].as_str().unwrap_or("standalone");
    let mut me = json!({"short": "this server", "status": "up", "role": role, "detail": detail});
    if role == "primary" {
        me["takes_writes"] = json!(true);
    }
    let mut copies = vec![me];
    for f in rep["replicas"].as_array().cloned().unwrap_or_default() {
        let gone = f["error"].is_string() && f["in_sync"] != true;
        copies.push(json!({"short": f["url"], "url": f["url"], "status": if gone { "down" } else { "up" }, "role": "replica"}));
    }
    let reads = cap["serves_reads"] != false;
    let writes = cap["takes_writes"] != false || role == "replica";
    let gone: Vec<String> = copies.iter().filter(|r| down(r)).map(short).collect();
    let (state, why) = if !reads {
        ("no_reads", cap["no_reads"].as_str().unwrap_or("this server serves no reads").to_string())
    } else if !writes {
        ("no_writes", cap["no_writes"].as_str().unwrap_or("this server takes no writes").to_string())
    } else if !gone.is_empty() {
        ("degraded", format!("{} of its replicas {} not answer; this server serves reads and takes writes", gone.len(), if gone.len() == 1 { "does" } else { "do" }))
    } else {
        ("ok", "This server serves reads and takes writes".to_string())
    };
    let n = copies.len();
    let up = n - gone.len();
    json!({"shards": [{"shard": 0, "state": state, "why": why, "serves_reads": reads, "takes_writes": writes, "majority": up * 2 > n,
        "failover": cap["failover_after_ms"].as_u64().unwrap_or(0) > 0, "primary": null, "replicas": copies}]})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(short: &str, up: bool) -> Json {
        if up {
            json!({"short": short, "url": format!("http://{short}"), "status": "ok", "detail": {"load": {"disk_used_ratio": 0.5}}})
        } else {
            json!({"short": short, "url": format!("http://{short}"), "status": "down", "down_since_ms": 1000})
        }
    }

    fn inputs<'a>(cluster: &'a Json, points: &'a [Arc<Json>]) -> Inputs<'a> {
        Inputs { cluster, points, slo_error_ratio: 0.01, slo_p99_ms: 250.0, alerts: (0, 0), partial: false, coordinator: Some(json!({"url": null, "name": "c"})), now_ms: 10_000 }
    }

    #[test]
    fn a_shard_without_majority_is_the_headline() {
        let c = json!({"shards": [
            {"shard": 0, "state": "ok", "failover": true, "majority": true, "replicas": [copy("s0-0", true), copy("s0-1", true)]},
            {"shard": 1, "state": "no_reads", "failover": true, "majority": false, "why": "1 of 2 copies answers: the primary has no majority, so it serves no reads", "replicas": [copy("s1-0", true), copy("s1-1", false)]},
        ]});
        let f = Fleet::default();
        let j = f.judge(inputs(&c, &[]));
        assert_eq!(j["status"], "critical");
        assert_eq!(j["headline"], "Searches are failing: shard 1 has no majority");
        assert_eq!(j["since_ms"], 1000);
        let p = &j["problems"][0];
        assert_eq!(p["where"], json!(["shard 1", "s1-1"]));
        assert_eq!(p["next"]["text"], "Bring s1-1 back, or remove it so s1-0 has a majority");
        assert_eq!(p["next"]["params"]["shard"], 1);
        assert_eq!(p["impact"], "Every search fails: each one needs shard 1");
        let shards = j["checks"].as_array().unwrap().iter().find(|c| c["id"] == "shards").unwrap().clone();
        assert_eq!(shards["value"], json!({"down": 0, "no_reads": 1, "no_writes": 0, "degraded": 0, "total": 2}));
        let copies = j["checks"].as_array().unwrap().iter().find(|c| c["id"] == "copies").unwrap().clone();
        assert_eq!((copies["down"].as_u64(), copies["total"].as_u64()), (Some(1), Some(4)));
    }

    /// 1 of 2 copies answering is no majority: no election can choose a
    /// primary, so the advice is not to wait for one.
    #[test]
    fn a_shard_of_two_with_one_down_cannot_elect() {
        let c = json!({"shards": [
            {"shard": 0, "state": "no_writes", "failover": true, "majority": false, "primary": null, "why": "1 of 2 copies answers: no copy takes writes", "replicas": [copy("s0-0", false), copy("s0-1", true)]},
        ]});
        let j = Fleet::default().judge(inputs(&c, &[]));
        let p = &j["problems"][0];
        assert_eq!(p["title"], "Shard 0 cannot elect a primary or take writes until a second copy answers");
        let next = p["next"]["text"].as_str().unwrap();
        assert!(next.starts_with("Bring s0-0 back, or, if it is gone for good, make s0-1 the primary by hand"), "{next}");
        assert!(!next.contains("within seconds"), "{next}");
        // With a majority, an election is under way.
        let c = json!({"shards": [
            {"shard": 0, "state": "no_writes", "failover": true, "majority": true, "primary": null, "why": "", "replicas": [copy("s0-0", false), copy("s0-1", true), copy("s0-2", true)]},
        ]});
        let j = Fleet::default().judge(inputs(&c, &[]));
        assert!(j["problems"][0]["next"]["text"].as_str().unwrap().contains("within seconds"));
    }

    #[test]
    fn the_error_ratio_counts_server_failures_over_the_window() {
        let c = json!({"shards": [{"shard": 0, "state": "ok", "replicas": [copy("s0-0", true)]}]});
        let pts: Vec<Arc<Json>> = (0..10u64).map(|k| Arc::new(json!({"t": k * 2000, "secs": 2.0, "queries": 100, "query_errors": if k >= 5 { 5 } else { 0 }, "latency_hist": [0, 0, 0, 0, 0, 100]}))).collect();
        let j = Fleet::default().judge(inputs(&c, &pts));
        let er = j["checks"].as_array().unwrap().iter().find(|c| c["id"] == "error_ratio").unwrap().clone();
        assert_eq!(er["value"], 0.025);
        assert_eq!(er["status"], "critical");
        assert_eq!(er["since_ms"], 10_000, "since the first point of the run over half the SLO");
        assert_eq!(j["headline"], "Searches are failing: 2.5% failed over the last 5 min (SLO 1%)");
        assert!(j["coordinators"][0]["qps"].as_f64().unwrap() == 50.0);
        let p99 = j["checks"].as_array().unwrap().iter().find(|c| c["id"] == "p99").unwrap().clone();
        assert!(p99["value_ms"].as_f64().unwrap() <= 50.0 && p99["status"] == "ok", "{p99}");
    }

    #[test]
    fn all_well_says_so_and_holds_since_the_status_began() {
        let c = json!({"shards": [{"shard": 0, "state": "ok", "replicas": [copy("s0-0", true), copy("s0-1", true)]}]});
        let f = Fleet::default();
        let j = f.judge(inputs(&c, &[]));
        assert_eq!(j["status"], "ok");
        assert_eq!(j["headline"], "Its 1 shard serves reads and takes writes; 2 of 2 copies are up. No search ended in the last 5 min");
        let mut later = inputs(&c, &[]);
        later.now_ms = 20_000;
        assert_eq!(f.judge(later)["since_ms"], 10_000);
        assert!(j["problems"].as_array().unwrap().is_empty());
    }
}
