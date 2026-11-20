//! What the screens show, read from the server's answers the way the web
//! command center reads them (`ui/app/views/cluster.js`'s `model`), so that
//! a copy has the same name, role and numbers in both.

use serde_json::Value as Json;

/// One copy of a shard, as `/v1/cluster` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Copy {
    pub url: String,
    /// The name the server's own texts use: its `short`, else `s<shard>-<k>`.
    pub id: String,
    /// The server's name (`--name`), when it has one.
    pub name: Option<String>,
    pub shard: u64,
    /// Its node (`--node`), else its host and port.
    pub node: String,
    pub zone: Option<String>,
    pub down: bool,
    pub primary: bool,
    pub role: &'static str,
    pub term: Option<u64>,
    pub txn: Option<u64>,
    /// Transactions it has still to apply from its primary (0 on the primary).
    pub lag: Option<f64>,
    pub copying: bool,
    pub p99: Option<f64>,
    pub qps: Option<f64>,
    /// CPU as a share of its cores.
    pub cpu: Option<f64>,
    pub disk: Option<f64>,
    /// The share of its searches that failed over its last 2 s.
    pub err: Option<f64>,
    pub version: Option<String>,
    pub rows: Option<u64>,
    /// Why the coordinator sends it no searches, when it does not.
    pub avoided: Option<String>,
    /// Why it does not answer, when it does not.
    pub error: Option<String>,
    pub down_since_ms: Option<u64>,
}

/// One shard and its copies.
#[derive(Clone, Debug, PartialEq)]
pub struct Shard {
    pub i: u64,
    /// `ok`, `degraded`, `no_reads`, `no_writes` or `down`.
    pub state: String,
    pub why: String,
    pub slots: Option<u64>,
    pub failover: Option<bool>,
    pub copies: Vec<Copy>,
    pub up: usize,
}

impl Shard {
    pub fn n(&self) -> usize {
        self.copies.len()
    }

    pub fn primary(&self) -> Option<&Copy> {
        self.copies.iter().find(|c| c.primary)
    }
}

fn f(j: &Json) -> Option<f64> {
    j.as_f64().filter(|v| v.is_finite())
}

fn s(j: &Json) -> Option<String> {
    j.as_str().filter(|s| !s.is_empty()).map(String::from)
}

/// `host:port` of a URL.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?']).next().unwrap_or(rest).to_string()
}

/// The shards of a `/v1/cluster` answer (none for a single server).
pub fn shards(c: &Json) -> Vec<Shard> {
    let Some(list) = c["shards"].as_array() else { return Vec::new() };
    list.iter()
        .enumerate()
        .map(|(idx, sh)| {
            let i = sh["shard"].as_u64().unwrap_or(idx as u64);
            let reps: &[Json] = sh["replicas"].as_array().map_or(&[], Vec::as_slice);
            let is_down = |r: &Json| r["status"] == "down" || r["role"] == "unknown";
            // The primary: a copy that answers and takes writes, the latest
            // term first (a dead old primary can still say it does), or the
            // one the shard names.
            let mut p: Option<usize> = reps.iter().enumerate().filter(|(_, r)| !is_down(r) && r["takes_writes"] == true).max_by_key(|(_, r)| r["term"].as_u64().unwrap_or(0)).map(|(k, _)| k);
            if let Some(named) = sh["primary"].as_str() {
                if let Some(k) = reps.iter().position(|r| !is_down(r) && r["url"] == named) {
                    p = Some(k);
                }
            }
            let follow: Vec<&Json> = p.and_then(|k| reps[k]["detail"]["health"]["replication"]["replicas"].as_array()).map(|a| a.iter().collect()).unwrap_or_default();
            let ptxn = p.and_then(|k| reps[k]["txn"].as_u64());
            let copies: Vec<Copy> = reps
                .iter()
                .enumerate()
                .map(|(k, r)| {
                    let down = is_down(r);
                    let d = &r["detail"];
                    let (l, mn) = if down { (&Json::Null, &Json::Null) } else { (&d["load"], &d["minute"]) };
                    let cores = f(&d["system"]["cores"]).or_else(|| f(&l["cores"])).or_else(|| f(&mn["cores"])).filter(|c| *c > 0.0);
                    let cpu = if down {
                        None
                    } else if let Some(b) = f(&mn["busy"]) {
                        Some(b)
                    } else {
                        f(&mn["cpu"]).or_else(|| f(&l["cpu"])).zip(cores).map(|(c, n)| c / n)
                    };
                    let primary = Some(k) == p;
                    let url = r["url"].as_str().unwrap_or_default().to_string();
                    let fw = follow.iter().find(|x| x["url"] == url.as_str());
                    let copying = !primary && fw.is_some_and(|x| x["copying"] == true);
                    let txn = r["txn"].as_u64();
                    let lag = if primary {
                        Some(0.0)
                    } else if down || p.is_none() {
                        None
                    } else if let Some(x) = fw {
                        if x["in_sync"] == true && !copying { Some(0.0) } else { f(&x["behind"]) }
                    } else {
                        ptxn.zip(txn).map(|(a, b)| a.saturating_sub(b) as f64)
                    };
                    let no_reads = !down && d["capability"]["serves_reads"] == false;
                    let qps_l = f(&l["qps"]);
                    Copy {
                        // Its `--node-name`, as the command center says it (OLU-402).
                        id: if r["short"].is_string() || r["name"].is_string() { crate::coord::shown(r) } else { format!("s{i}-{k}") },
                        name: s(&r["name"]),
                        shard: i,
                        node: s(&r["node"]).unwrap_or_else(|| host_of(&url)),
                        zone: s(&r["zone"]),
                        down,
                        primary,
                        role: if down { "down" } else if primary { "primary" } else { "replica" },
                        term: r["term"].as_u64().or_else(|| d["health"]["replication"]["term"].as_u64()),
                        txn,
                        lag,
                        copying,
                        p99: if down || no_reads { None } else { f(&mn["p99_ms"]).or_else(|| f(&l["p99_ms"])) },
                        qps: if down { None } else { f(&mn["qps"]).or(qps_l) },
                        cpu,
                        disk: if down { None } else { f(&l["disk_used_ratio"]).or_else(|| f(&mn["disk_used_ratio"])) },
                        err: match qps_l {
                            Some(q) if !down && q >= 1.0 => Some(f(&l["errors_s"]).unwrap_or(0.0) / q),
                            _ => None,
                        },
                        version: s(&r["version"]["version"]).or_else(|| s(&d["system"]["version"])),
                        rows: d["health"]["rows"].as_u64().or_else(|| r["docs"].as_u64()),
                        avoided: if r["avoided"] == true { Some(s(&r["avoided_why"]).unwrap_or_else(|| "after failures".into())) } else { None },
                        error: s(&r["error"]),
                        down_since_ms: r["down_since_ms"].as_u64(),
                        url,
                    }
                })
                .collect();
            let n = copies.len();
            let up = copies.iter().filter(|c| !c.down).count();
            let majority = sh["majority"].as_bool().unwrap_or(up * 2 > n);
            let state = s(&sh["state"]).unwrap_or_else(|| (if up == 0 { "down" } else if !majority { "no_writes" } else if up < n { "degraded" } else { "ok" }).into());
            let why = s(&sh["why"]).unwrap_or_else(|| match state.as_str() {
                "down" => format!("None of its {n} copies answers: searches that need it fail and it takes no writes."),
                _ if !majority => format!("{up} of {n} copies answer, fewer than a majority: it takes no writes."),
                _ if up < n => format!("{} of {n} copies do not answer: it serves from the other {up}.", n - up),
                _ => format!("{up} of {n} copies answer."),
            });
            Shard { i, state, why, slots: sh["slots_held"].as_u64(), failover: sh["failover"].as_bool(), copies, up }
        })
        .collect()
}

/// Why a copy cannot be made its shard's primary now, as the web says it
/// (`copyActions`), or `None` when it can.
pub fn why_not_primary(sh: &Shard, c: &Copy) -> Option<String> {
    if c.primary {
        Some(format!("{} is the primary already: pick a replica", c.id))
    } else if c.down {
        Some(format!("{} does not answer", c.id))
    } else if c.copying {
        Some(format!("{} is being copied", c.id))
    } else if c.lag.is_some_and(|l| l > 0.0) {
        Some(format!("{} is {} transactions behind: wait until it catches up", c.id, c.lag.unwrap_or(0.0)))
    } else if sh.n() < 2 {
        Some(format!("shard {} has one copy", sh.i))
    } else {
        None
    }
}

/// How a screen colours a number: what it says about the thing measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Bad,
}

/// What the cluster map colours copies and shards by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lens {
    Health,
    P99,
    Cpu,
    Disk,
    Errors,
    Lag,
    Version,
}

pub const LENSES: [Lens; 7] = [Lens::Health, Lens::P99, Lens::Cpu, Lens::Disk, Lens::Errors, Lens::Lag, Lens::Version];

/// Replica lag (transactions) past which the map turns amber, and red.
const LAG_WARN: f64 = 1000.0;
const LAG_BAD: f64 = 10000.0;

impl Lens {
    pub fn name(self) -> &'static str {
        match self {
            Lens::Health => "health",
            Lens::P99 => "p99",
            Lens::Cpu => "cpu",
            Lens::Disk => "disk",
            Lens::Errors => "errors",
            Lens::Lag => "lag",
            Lens::Version => "version",
        }
    }

    pub fn parse(s: &str) -> Option<Lens> {
        LENSES.into_iter().find(|l| l.name().eq_ignore_ascii_case(s))
    }

    pub fn next(self) -> Lens {
        let i = LENSES.iter().position(|l| *l == self).unwrap_or(0);
        LENSES[(i + 1) % LENSES.len()]
    }

    /// The copy's number this lens shows.
    pub fn value(self, c: &Copy) -> Option<f64> {
        match self {
            Lens::P99 => c.p99,
            Lens::Cpu => c.cpu,
            Lens::Disk => c.disk,
            Lens::Errors => c.err,
            Lens::Lag if c.primary => None,
            Lens::Lag => c.lag,
            Lens::Health | Lens::Version => None,
        }
    }

    /// The worst of a shard's copies that answer.
    pub fn shard_value(self, sh: &Shard) -> Option<f64> {
        sh.copies.iter().filter(|c| !c.down).filter_map(|c| self.value(c)).reduce(f64::max)
    }

    /// Amber and red, against the cluster's SLOs where the lens has one.
    pub fn level(self, v: Option<f64>, slo: &Slo) -> Level {
        let Some(v) = v else { return Level::Ok };
        let (warn, bad) = match self {
            Lens::P99 => (slo.p99_ms * 0.6, slo.p99_ms),
            Lens::Cpu => (0.7, 0.85),
            Lens::Disk => (0.8, 0.9),
            Lens::Errors => (slo.error_ratio / 4.0, slo.error_ratio),
            Lens::Lag => (LAG_WARN, LAG_BAD),
            Lens::Health | Lens::Version => return Level::Ok,
        };
        if v >= bad {
            Level::Bad
        } else if v >= warn {
            Level::Warn
        } else {
            Level::Ok
        }
    }

    pub fn fmt(self, v: Option<f64>) -> String {
        let Some(v) = v else { return "–".into() };
        match self {
            Lens::P99 => ms(v),
            Lens::Cpu | Lens::Disk => format!("{:.0}%", v * 100.0),
            Lens::Errors => pct(v),
            Lens::Lag => format!("{} txn", count(v)),
            Lens::Health | Lens::Version => String::new(),
        }
    }
}

/// The SLOs the lenses measure against, from the fleet's health.
#[derive(Clone, Debug, PartialEq)]
pub struct Slo {
    pub p99_ms: f64,
    pub error_ratio: f64,
}

impl Slo {
    pub fn of(fleet: &Json, cluster: &Json) -> Slo {
        let check = |id: &str| fleet["checks"].as_array().and_then(|a| a.iter().find(|c| c["id"] == id)).cloned().unwrap_or(Json::Null);
        Slo {
            p99_ms: f(&check("p99")["slo_ms"]).or_else(|| f(&cluster["cluster"]["slo_p99_ms"])).unwrap_or(250.0),
            error_ratio: f(&check("error_ratio")["slo"]).unwrap_or(0.01),
        }
    }
}

impl Default for Slo {
    fn default() -> Slo {
        Slo { p99_ms: 250.0, error_ratio: 0.01 }
    }
}

/// A shard's health, as the map colours it.
pub fn health(sh: &Shard) -> Level {
    match sh.state.as_str() {
        "down" | "no_reads" | "no_writes" => Level::Bad,
        "ok" if sh.copies.iter().all(|c| c.avoided.is_none() && c.lag.is_none_or(|l| l < LAG_WARN)) => Level::Ok,
        _ => Level::Warn,
    }
}

/// The version most copies run: the others stand out on the version lens.
pub fn usual_version(shards: &[Shard]) -> Option<String> {
    let mut seen: Vec<(String, usize)> = Vec::new();
    for v in shards.iter().flat_map(|s| &s.copies).filter_map(|c| c.version.clone()) {
        match seen.iter_mut().find(|(x, _)| *x == v) {
            Some(e) => e.1 += 1,
            None => seen.push((v, 1)),
        }
    }
    seen.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v)
}

/// The nodes copies are on, each with its copies, in the order first seen.
pub fn nodes(shards: &[Shard]) -> Vec<(String, Option<String>, Vec<Copy>)> {
    let mut out: Vec<(String, Option<String>, Vec<Copy>)> = Vec::new();
    for c in shards.iter().flat_map(|s| &s.copies) {
        match out.iter_mut().find(|(n, _, _)| *n == c.node) {
            Some(e) => e.2.push(c.clone()),
            None => out.push((c.node.clone(), c.zone.clone(), vec![c.clone()])),
        }
    }
    out
}

/// Whether the server is production, as the web decides (`data.js`'s
/// `isProduction`): an environment that does not name a test or a
/// sandbox, or none on a server that is not this machine's or is reached
/// through a hub.
/// The log tables in `GET /v1/tables` (the tables the key reads): those
/// with a `TIMESTAMP`, as the web's Log explorer finds them, each with its
/// time column (`ts` when it has one, else its first `TIMESTAMP`).
pub fn log_tables(tables: &Json) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = tables["tables"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let times: Vec<&str> = t["short"]["fields"].as_array()?.iter().filter(|f| f["type"] == "timestamp").filter_map(|f| f["name"].as_str()).collect();
            let ts = times.iter().find(|n| **n == "ts").or(times.first())?;
            Some((t["table"].as_str()?.to_string(), ts.to_string()))
        })
        .collect();
    out.sort();
    out
}

/// The newest lines of log table `table` by its time column `ts`, as the
/// Log explorer's first look: the last 15 minutes.
pub fn newest_lines_sql(table: &str, ts: &str) -> String {
    format!("SELECT * FROM {table} WHERE {ts} > now() - INTERVAL '15 minutes' ORDER BY {ts} DESC LIMIT 100")
}

pub fn is_production(environment: Option<&str>, base: &str) -> bool {
    match environment.filter(|e| !e.trim().is_empty()) {
        Some(e) => {
            let e = e.trim().to_ascii_lowercase();
            let word = e.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or_default();
            !matches!(word, "dev" | "development" | "test" | "testing" | "stage" | "staging" | "qa" | "local" | "sandbox" | "demo")
        }
        None => !is_sandbox(base),
    }
}

/// No environment set, on this machine, not through a hub.
pub fn is_sandbox(base: &str) -> bool {
    let rest = base.split_once("://").map_or(base, |(_, r)| r);
    let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    let local = host.eq_ignore_ascii_case("localhost") || host == "::1" || host.starts_with("127.");
    local && !path.starts_with("c/")
}

// ---------------------------------------------------------------- numbers and times

/// Milliseconds, with the digits that matter.
pub fn ms(v: f64) -> String {
    if v >= 10_000.0 {
        format!("{:.1}s", v / 1000.0)
    } else if v >= 10.0 {
        format!("{v:.0}ms")
    } else if v >= 1.0 {
        format!("{v:.1}ms")
    } else {
        format!("{v:.2}ms")
    }
}

pub fn pct(v: f64) -> String {
    if v > 0.0 && v < 0.01 {
        format!("{:.2}%", v * 100.0)
    } else if v < 0.1 {
        format!("{:.1}%", v * 100.0)
    } else {
        format!("{:.0}%", v * 100.0)
    }
}

/// 1234 as 1,234; a million as 1.2M.
pub fn count(v: f64) -> String {
    let v = v.max(0.0);
    if v >= 10_000_000.0 {
        format!("{:.0}M", v / 1e6)
    } else if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1e6)
    } else {
        let n = v.round() as u64;
        let s = n.to_string();
        let mut out = String::new();
        for (k, ch) in s.chars().enumerate() {
            if k > 0 && (s.len() - k) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        out
    }
}

pub fn bytes(v: f64) -> String {
    let v = v.max(0.0);
    for (unit, size) in [("TB", 1e12), ("GB", 1e9), ("MB", 1e6), ("KB", 1e3)] {
        if v >= size {
            return format!("{:.1} {unit}", v / size);
        }
    }
    format!("{v:.0} B")
}

/// A time of day on this machine's clock, `11:27:05 PM`.
pub fn clock(ms: u64) -> String {
    let (h, m, s) = local_hms(ms);
    let (h12, half) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{h12}:{m:02}:{s:02} {half}")
}

/// How long ago, in words a glance takes in: `12 s`, `4 min`, `3 h`, `2 d`.
pub fn ago(now_ms: u64, ms: u64) -> String {
    let s = now_ms.saturating_sub(ms) / 1000;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        3600..=172_799 => format!("{} h", s / 3600),
        _ => format!("{} d", s / 86_400),
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The zone this machine's clock is in, as it abbreviates it (`PST`).
pub fn zone() -> String {
    #[cfg(unix)]
    {
        if let Some(z) = local_tm(now_ms()).and_then(|tm| {
            if tm.tm_zone.is_null() {
                return None;
            }
            // SAFETY: tm_zone points at a NUL-terminated name the C library keeps.
            Some(unsafe { std::ffi::CStr::from_ptr(tm.tm_zone) }.to_string_lossy().into_owned())
        }) {
            return z;
        }
    }
    "UTC".into()
}

#[cfg(unix)]
fn local_tm(ms: u64) -> Option<libc::tm> {
    let t = (ms / 1000) as libc::time_t;
    // SAFETY: localtime_r writes the broken-down time into `tm`, which we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    ok.then_some(tm)
}

fn local_hms(ms: u64) -> (u64, u64, u64) {
    #[cfg(unix)]
    {
        if let Some(tm) = local_tm(ms) {
            return (tm.tm_hour as u64, tm.tm_min as u64, tm.tm_sec as u64);
        }
    }
    let s = ms / 1000;
    ((s / 3600) % 24, (s / 60) % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cluster() -> Json {
        json!({"shards": [
            {"shard": 0, "state": "ok", "slots_held": 2048, "primary": "http://a:7711", "replicas": [
                {"url": "http://a:7711", "short": "s0-0", "role": "primary", "takes_writes": true, "term": 2, "txn": 10, "status": "ok",
                 "detail": {"minute": {"busy": 0.5, "p99_ms": 12.0, "qps": 30.0}, "load": {"disk_used_ratio": 0.82, "qps": 2.0, "errors_s": 1.0},
                            "health": {"replication": {"replicas": [{"url": "http://b:7711", "in_sync": true}, {"url": "http://c:7711", "in_sync": false, "behind": 1500}]}},
                            "system": {"version": "0.3.4"}}},
                {"url": "http://b:7711", "role": "replica", "takes_writes": false, "term": 2, "txn": 10, "status": "ok", "node": "n2",
                 "detail": {"minute": {"busy": 0.1, "p99_ms": 300.0}, "system": {"version": "0.3.4"}}},
                {"url": "http://c:7711", "role": "replica", "takes_writes": false, "term": 2, "txn": 9, "status": "ok", "detail": {"system": {"version": "0.3.3"}}}
            ]},
            {"shard": 1, "replicas": [
                {"url": "http://d:7711", "role": "primary", "takes_writes": true, "term": 1, "status": "down", "error": "connection refused"},
                {"url": "http://e:7711", "role": "replica", "takes_writes": false, "term": 1, "status": "ok"}
            ]}
        ]})
    }

    #[test]
    fn copies_are_named_and_placed_as_the_web_names_them() {
        let sh = shards(&cluster());
        assert_eq!(sh.len(), 2);
        let ids: Vec<&str> = sh[0].copies.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["s0-0", "s0-1", "s0-2"]);
        assert_eq!(sh[0].copies[1].node, "n2");
        assert_eq!(sh[0].copies[2].node, "c:7711");
        assert!(sh[0].copies[0].primary && sh[0].copies[0].role == "primary");
        assert_eq!(sh[0].copies[1].lag, Some(0.0));
        assert_eq!(sh[0].copies[2].lag, Some(1500.0));
        assert_eq!(sh[0].copies[0].err, Some(0.5));
    }

    /// OLU-402: a copy is named by its `--node-name`, as the web names it;
    /// an operator's pod keeps its short name.
    #[test]
    fn copies_are_named_by_their_node_name() {
        let c = json!({"shards": [{"shard": 1, "replicas": [
            {"url": "http://127.0.0.1:17754", "short": "127.0.0.1:17754", "name": "s1a", "status": "down", "role": "unknown"},
            {"url": "http://127.0.0.1:17755", "short": "127.0.0.1:17755", "role": "primary", "takes_writes": true, "status": "ok"},
            {"url": "http://x-s1-2.x:7701", "short": "s1-2", "name": "x-s1-2", "role": "replica", "status": "ok"}
        ]}]});
        let ids: Vec<String> = shards(&c)[0].copies.iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids, ["s1a", "127.0.0.1:17755", "s1-2"]);
    }

    #[test]
    fn a_dead_primary_is_not_the_primary() {
        let sh = shards(&cluster());
        let s1 = &sh[1];
        assert!(s1.primary().is_none());
        assert!(s1.copies[0].down && s1.copies[0].role == "down");
        assert_eq!(s1.up, 1);
        assert_eq!(s1.state, "no_writes");
        assert_eq!(health(s1), Level::Bad);
        assert_eq!(s1.copies[1].lag, None);
    }

    #[test]
    fn a_handover_is_refused_for_what_the_web_refuses_it() {
        let sh = shards(&cluster());
        assert!(why_not_primary(&sh[0], &sh[0].copies[0]).unwrap().contains("primary already"));
        assert_eq!(why_not_primary(&sh[0], &sh[0].copies[1]), None);
        assert!(why_not_primary(&sh[0], &sh[0].copies[2]).unwrap().contains("1500 transactions behind"));
        assert!(why_not_primary(&sh[1], &sh[1].copies[0]).unwrap().contains("does not answer"));
    }

    #[test]
    fn lenses_measure_against_the_slos() {
        let sh = shards(&cluster());
        let slo = Slo::default();
        assert_eq!(Lens::P99.shard_value(&sh[0]), Some(300.0));
        assert_eq!(Lens::P99.level(Some(300.0), &slo), Level::Bad);
        assert_eq!(Lens::P99.level(Some(160.0), &slo), Level::Warn);
        assert_eq!(Lens::Disk.level(Some(0.82), &slo), Level::Warn);
        assert_eq!(Lens::Lag.shard_value(&sh[0]), Some(1500.0));
        assert_eq!(Lens::Lag.fmt(Some(1500.0)), "1,500 txn");
        assert_eq!(usual_version(&sh).as_deref(), Some("0.3.4"));
        assert_eq!(Lens::parse("P99"), Some(Lens::P99));
        assert_eq!(Lens::Version.next(), Lens::Health);
    }

    #[test]
    fn production_is_what_the_web_calls_production() {
        assert!(is_production(Some("production"), "http://127.0.0.1:7700"));
        assert!(!is_production(Some("staging-2"), "https://x"));
        assert!(!is_production(None, "http://127.0.0.1:7700"));
        assert!(!is_production(None, "http://localhost:7700/"));
        assert!(is_production(None, "http://127.0.0.1:7700/c/prod"));
        assert!(is_production(None, "https://db.example.com"));
        assert!(!is_production(None, "http://[::1]:7700"));
    }

    #[test]
    fn numbers_read_at_a_glance() {
        assert_eq!(count(1234567.0), "1.2M");
        assert_eq!(count(20000.0), "20,000");
        assert_eq!(ms(3.47), "3.5ms");
        assert_eq!(ms(0.4), "0.40ms");
        assert_eq!(ms(12_500.0), "12.5s");
        assert_eq!(bytes(10_514_984.0), "10.5 MB");
        assert_eq!(pct(0.005), "0.50%");
        assert_eq!(ago(100_000, 40_000), "1 min");
        assert_eq!(ago(100_000, 99_000), "1 s");
        assert_eq!(host_of("http://127.0.0.1:7711/x"), "127.0.0.1:7711");
    }
}
