//! Alert rules, each a SQL statement kept in `bb_rules` and run every
//! minute over the newest rows; the rows a rule returns are the alerts.
//! An alert stays open while its rule finds it, and ends when it stops.
//! Open alerts that share something (whose space, who announced it, which
//! country) are grouped into incidents.
//!
//! Every statement reads a window that ends at the newest row the table
//! holds, not at the clock, so an archive read hours late finds what was
//! wrong then.

use std::collections::BTreeMap;

use anyhow::Result;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Value};

use crate::strata::{Row, RowExt, Strata, Trace};

pub struct Rule {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: &'static str,
    pub severity: &'static str,
    /// How often it runs, in seconds (a run of the rules is each minute).
    pub every_s: u64,
    pub about: &'static str,
    pub sql: &'static str,
}

/// `:since` is the window's start, `:collectors` how many collectors the
/// feed reads (thresholds of counts grow with it).
pub const RULES: &[Rule] = &[
    Rule {
        id: "rpki-invalid",
        title: "RPKI-invalid route seen by 3 or more peers",
        kind: "invalid",
        severity: "bad",
        every_s: 60,
        about: "An origin announced a prefix that a ROA gives to another network (or not so long): a mistake, a lease whose ROA did not move, or a hijack. Only lengths the internet routes (IPv4 /24, IPv6 /48 or shorter): a /32 is most often a network blackholing one of its own addresses, which the blackhole rule shows.",
        sql: "SELECT prefix, origin, owner, COUNT(DISTINCT peer) AS peers, COUNT(*) AS updates, MIN(ts) AS first, MAX(ts) AS last
FROM bb_updates WHERE ts >= :since AND kind = 'A' AND origin > 0 AND rpki IN ('invalid', 'invalid-length')
  AND ((af = 4 AND plen <= 24) OR (af = 6 AND plen <= 48))
GROUP BY prefix, origin, owner HAVING COUNT(DISTINCT peer) >= 3 ORDER BY peers DESC LIMIT 300",
    },
    Rule {
        id: "new-origin",
        title: "A prefix announced by a new, unrelated origin",
        kind: "origin",
        severity: "warn",
        every_s: 60,
        about: "RouteViews saw this prefix from another network yesterday, the two are not customer and provider, and no ROA says which is right.",
        sql: "SELECT prefix, origin, owner, COUNT(DISTINCT peer) AS peers, COUNT(*) AS updates, MIN(ts) AS first, MAX(ts) AS last
FROM bb_updates WHERE ts >= :since AND kind = 'A' AND origin > 0 AND seen = 'new-origin' AND rpki = 'unknown'
  AND ((af = 4 AND plen <= 24) OR (af = 6 AND plen <= 48))
GROUP BY prefix, origin, owner HAVING COUNT(DISTINCT peer) >= 10 ORDER BY peers DESC LIMIT 100",
    },
    Rule {
        id: "more-specific",
        title: "A longer prefix inside another network's space",
        kind: "subprefix",
        severity: "warn",
        every_s: 60,
        about: "A prefix no one announced yesterday, inside space another network announces, from an origin that is not its customer and no ROA covers: how a sub-prefix hijack looks, and also how many new customers do.",
        sql: "SELECT prefix, origin, owner, COUNT(DISTINCT peer) AS peers, COUNT(*) AS updates, MIN(ts) AS first, MAX(ts) AS last
FROM bb_updates WHERE ts >= :since AND kind = 'A' AND origin > 0 AND seen = 'more-specific' AND rpki = 'unknown'
  AND ((af = 4 AND plen <= 24) OR (af = 6 AND plen <= 48))
GROUP BY prefix, origin, owner HAVING COUNT(DISTINCT peer) >= 20 ORDER BY peers DESC LIMIT 100",
    },
    Rule {
        id: "flapping",
        title: "A prefix announced and withdrawn thousands of times an hour",
        kind: "flap",
        severity: "warn",
        // It counts an hour of every prefix's updates, the dearest of the
        // rules: every five minutes. Its peers are counted afterwards for
        // the prefixes it finds (`peers_of`): a distinct count for each of
        // 300,000 prefixes took 8 to 10 s and 7 GB, without it 2 s and 1 GB
        // (6 million updates, 18 November 2026).
        every_s: 300,
        about: "Every change of a route is sent across the internet; a prefix that changes this often costs every router that hears it, and its network's users see a link that comes and goes.",
        sql: "SELECT prefix, MAX(origin) AS origin, MAX(owner) AS owner, COUNT(*) AS updates, COUNT(*) FILTER (WHERE kind = 'W') AS withdrawn,
  MIN(ts) AS first, MAX(ts) AS last
FROM bb_updates WHERE ts >= :since_hour GROUP BY prefix HAVING COUNT(*) >= 10000 * :collectors ORDER BY updates DESC LIMIT 20",
    },
    Rule {
        id: "withdrawn",
        title: "A network's prefixes withdrawn by many peers at once",
        kind: "outage",
        severity: "bad",
        every_s: 60,
        about: "Five or more peers withdrew twenty or more of one network's prefixes in the window: the network, or its only way out, is gone from the routing table.",
        sql: "SELECT origin, COUNT(DISTINCT prefix) AS prefixes, COUNT(DISTINCT peer) AS peers, COUNT(*) AS updates, MIN(ts) AS first, MAX(ts) AS last
FROM bb_updates WHERE ts >= :since AND kind = 'W' AND origin IS NOT NULL
GROUP BY origin HAVING COUNT(DISTINCT prefix) >= 20 AND COUNT(DISTINCT peer) >= 5 ORDER BY prefixes DESC LIMIT 50",
    },
    Rule {
        id: "blackhole",
        title: "A prefix tagged for blackholing (65535:666)",
        kind: "blackhole",
        severity: "info",
        every_s: 60,
        about: "A network asked its neighbours to drop all traffic to this prefix, as networks do while one of their addresses is under a denial-of-service attack.",
        sql: "SELECT prefix, origin, owner, COUNT(DISTINCT peer) AS peers, COUNT(*) AS updates, MIN(ts) AS first, MAX(ts) AS last
FROM bb_updates WHERE ts >= :since AND kind = 'A' AND comms = '65535:666'
GROUP BY prefix, origin, owner ORDER BY peers DESC LIMIT 100",
    },
    Rule {
        id: "ioda",
        title: "A country or region IODA sees going dark (routes, pings or traffic)",
        kind: "outage",
        severity: "bad",
        every_s: 60,
        about: "IODA (Georgia Tech) watches every country, region and network: when its routes, the addresses that answer pings, or the traffic it sends fall well below normal, it opens an event. This rule takes the countries and regions whose event began in the last two days; a network's own outage is the withdrawn rule's.",
        sql: "SELECT id, kind, entity, asn, country, name, source, start, until, score FROM bb_outage
WHERE open AND kind IN ('country', 'region') AND start >= :since_2d ORDER BY score DESC LIMIT 12",
    },
    Rule {
        id: "slow-country",
        title: "A country whose probes reach the K root server slowly",
        kind: "slow",
        severity: "warn",
        every_s: 60,
        about: "Half or more of a country's RIPE Atlas probes take over 100 ms to reach the nearest K root server, though K has a node on most continents: their networks do not meet the node near them.",
        sql: "SELECT country, MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes, COUNT(*) FILTER (WHERE rtt > 100) AS slow, COUNT(*) AS pings
FROM bb_ping WHERE ts >= :since_atlas AND msm = 1001 AND rtt IS NOT NULL GROUP BY country
HAVING COUNT(DISTINCT probe) >= 5 AND MEDIAN(rtt) > 100 ORDER BY med DESC LIMIT 20",
    },
];

pub async fn install(db: &Strata) -> Result<()> {
    let rows: Vec<Value> = RULES
        .iter()
        .map(|r| json!({"id": r.id, "title": r.title, "kind": r.kind, "severity": r.severity, "every_s": r.every_s, "sql": r.sql, "about": r.about, "enabled": true}))
        .collect();
    db.upsert("bb_rules", rows).await.map_err(|e| anyhow::anyhow!("bb_rules: {e}"))?;
    Ok(())
}

fn iso(us: i64) -> String {
    chrono::DateTime::from_timestamp_micros(us).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
}

pub fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_micros())
}

/// How many peers sent each of `rows`' prefixes since `since`: one small
/// query over the prefixes a rule found, rather than a distinct count for
/// every prefix of the hour.
async fn peers_of(db: &Strata, trace: &Trace, rows: &mut [Row], since: &str) {
    let prefixes: Vec<String> = rows.iter().map(|r| r.s("prefix")).filter(|p| !p.is_empty()).collect();
    if prefixes.is_empty() {
        return;
    }
    let sql = "SELECT prefix, COUNT(DISTINCT peer) AS peers FROM bb_updates WHERE prefix IN (:p) AND ts >= :since GROUP BY prefix LIMIT 100";
    let got = match db.traced_within(trace, "flapping: its prefixes' peers", sql, json!({"p": prefixes, "since": since}), Some(60_000)).await {
        Ok(got) => got,
        Err(e) => {
            tracing::warn!(error = %e, "flapping: peers");
            return;
        }
    };
    let by: FxHashMap<String, i64> = got.iter().map(|x| (x.s("prefix"), x.i("peers"))).collect();
    for r in rows.iter_mut() {
        r.insert("peers".into(), json!(by.get(&r.s("prefix")).copied().unwrap_or(0)));
    }
}

/// The newest update's time, the newest ping's, and how many collectors
/// are being read.
///
/// The newest rows are read from where the last run found them (a MAX over
/// three days of updates costs seconds, and its deadline when the feed is
/// busy), and the collectors from the rollup of updates a minute rather
/// than two days of rows.
pub async fn clock(db: &Strata) -> (i64, i64, usize) {
    static SEEN: std::sync::Mutex<(i64, i64)> = std::sync::Mutex::new((0, 0));
    let (su, sp) = *SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let (u, p, c) = tokio::join!(
        db.query("SELECT MAX(ts) AS t FROM bb_updates WHERE ts >= :since", json!({"since": iso(su)})),
        db.query("SELECT MAX(ts) AS t FROM bb_ping WHERE ts >= :since", json!({"since": iso(sp)})),
        db.query("SELECT collector, SUM(count) AS n FROM bb_updates_1m WHERE bucket >= now() - INTERVAL '2 days' GROUP BY collector", Value::Null),
    );
    let t = |r: Result<Vec<Row>, crate::strata::Error>| r.ok().and_then(|r| r.first().and_then(|x| parse_ts(&x.s("t")))).unwrap_or(0);
    let n = c.map(|r| r.len()).unwrap_or(1).max(1);
    let wall = chrono::Utc::now().timestamp_micros() + 60_000_000;
    let (u, p) = (t(u).min(wall).max(su), t(p).min(wall).max(sp));
    *SEEN.lock().unwrap_or_else(|e| e.into_inner()) = (u, p);
    (u, p, n)
}

/// One run of every rule: alerts opened, kept and ended, then incidents.
pub async fn run_once(db: &Strata, names: &Names) -> Result<Value> {
    let trace = Trace::new();
    let (now_u, now_p, collectors) = clock(db).await;
    if now_u == 0 {
        return Ok(json!({"skipped": "no updates yet"}));
    }
    let since = iso(now_u - 15 * 60 * 1_000_000);
    let since_hour = iso(now_u - 3600 * 1_000_000);
    let since_atlas = iso(now_p.max(1) - 20 * 60 * 1_000_000);
    let since_2d = iso(now_u - 2 * 86400 * 1_000_000);
    let mut found: Vec<Value> = Vec::new();
    let mut ran: Vec<String> = Vec::new();
    // When each rule last ran: one not due keeps its alerts as they are.
    static LAST: std::sync::Mutex<Vec<(&str, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());
    for r in RULES {
        let due = {
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            let at = last.iter().find(|x| x.0 == r.id).map(|x| x.1);
            // A few seconds early is on time: the runs are a minute apart.
            let due = at.is_none_or(|t| t.elapsed().as_secs() + 10 >= r.every_s);
            if due {
                last.retain(|x| x.0 != r.id);
                last.push((r.id, std::time::Instant::now()));
            }
            due
        };
        if !due {
            continue;
        }
        let mut params = serde_json::Map::new();
        for (k, v) in [("since", json!(since)), ("since_hour", json!(since_hour)), ("since_atlas", json!(since_atlas)), ("since_2d", json!(since_2d)), ("collectors", json!(collectors))] {
            if r.sql.contains(&format!(":{k}")) {
                params.insert(k.into(), v);
            }
        }
        let t0 = std::time::Instant::now();
        let mut rows = match db.traced_within(&trace, r.id, r.sql, Value::Object(params), Some(60_000)).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(rule = r.id, error = %e, "rule");
                continue;
            }
        };
        if r.id == "flapping" {
            peers_of(db, &trace, &mut rows, &since_hour).await;
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let _ = db
            .write(vec![
                json!({"op": "set", "table": "bb_rules", "key": r.id, "field": "last_run", "value": iso(now_u)}),
                json!({"op": "set", "table": "bb_rules", "key": r.id, "field": "last_ms", "value": (ms * 10.0).round() / 10.0}),
                json!({"op": "set", "table": "bb_rules", "key": r.id, "field": "last_rows", "value": rows.len()}),
            ])
            .await;
        ran.push(r.id.to_string());
        for x in &rows {
            if let Some(a) = alert(r, x, names) {
                found.push(a);
            }
        }
    }
    // Withdrawals: only a network whose prefixes did not come back.
    let mut kept = Vec::new();
    for a in found {
        if a["rule"] == "withdrawn" {
            let o = a["origin"].as_i64().unwrap_or(0);
            let back = db
                .query("SELECT COUNT(DISTINCT prefix) AS n FROM bb_updates WHERE ts >= :t AND kind = 'A' AND origin = :o", json!({"t": a["last_seen"], "o": o}))
                .await
                .ok()
                .and_then(|r| r.first().map(|x| x.i("n")))
                .unwrap_or(0);
            if back * 2 >= a["value"].as_i64().unwrap_or(0) {
                continue;
            }
        }
        kept.push(a);
    }
    let open = store(db, kept, &ran, &iso(now_u)).await?;
    let incidents = group(db, names).await?;
    Ok(json!({"open": open, "incidents": incidents, "queries": trace.take()}))
}

/// Names and countries of networks, read once a while for titles.
#[derive(Default)]
pub struct Names {
    pub asn: FxHashMap<i64, (String, String)>,
    /// Each country's name by its code (from the page's map, CLDR's).
    pub cc: FxHashMap<String, String>,
}

impl Names {
    pub fn name(&self, a: i64) -> String {
        self.asn.get(&a).map(|x| x.0.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("AS{a}"))
    }
    pub fn country(&self, a: i64) -> String {
        self.asn.get(&a).map(|x| x.1.clone()).unwrap_or_default()
    }
    pub fn place(&self, cc: &str) -> String {
        self.cc.get(cc).cloned().unwrap_or_else(|| cc.to_string())
    }
    /// Country names from the page's map file.
    pub fn with_countries(mut self, geo: &str) -> Names {
        if let Ok(v) = serde_json::from_str::<Value>(geo) {
            for (k, c) in v["countries"].as_object().into_iter().flatten() {
                if let Some(name) = c["n"].as_str() {
                    self.cc.insert(k.clone(), name.to_string());
                }
            }
        }
        self
    }
    pub async fn read(db: &Strata, asns: &[i64]) -> Names {
        let mut n = Names::default();
        for chunk in asns.chunks(500) {
            if let Ok(rows) = db.query("SELECT asn, short, country FROM bb_as WHERE asn IN (:a)", json!({"a": chunk})).await {
                for r in rows {
                    n.asn.insert(r.i("asn"), (r.s("short"), r.s("country")));
                }
            }
        }
        n
    }
}

fn alert(r: &Rule, x: &Row, n: &Names) -> Option<Value> {
    let prefix = x.s("prefix");
    let origin = x.i("origin");
    let owner = x.i("owner");
    let (subject, title, detail, country, asn, value) = match r.id {
        "rpki-invalid" | "new-origin" | "more-specific" | "blackhole" => {
            let what = match r.id {
                "rpki-invalid" if origin == owner => format!("{} announced {prefix}, longer than its own ROA allows", n.name(origin)),
                "rpki-invalid" => format!("{} announced {}'s {prefix}", n.name(origin), n.name(owner)),
                "new-origin" => format!("{prefix} announced by {}, not its usual {}", n.name(origin), n.name(owner)),
                "more-specific" => format!("{} announced {prefix}, inside {}'s space", n.name(origin), n.name(owner)),
                _ => format!("{}'s {prefix} tagged for blackholing", n.name(origin)),
            };
            (format!("{prefix}|{origin}"), what, format!("{} peers, {} updates", x.i("peers"), x.i("updates")), n.country(owner.max(origin)), origin, x.f("peers"))
        }
        "flapping" => (
            prefix.clone(),
            format!("{}'s {prefix} changed {} times in an hour", n.name(origin), x.i("updates")),
            format!("withdrawn {} times, {} peers", x.i("withdrawn"), x.i("peers")),
            n.country(origin),
            origin,
            x.f("updates"),
        ),
        "withdrawn" => (
            format!("AS{origin}"),
            format!("{} lost {} prefixes", n.name(origin), x.i("prefixes")),
            format!("withdrawn by {} peers, {} updates", x.i("peers"), x.i("updates")),
            n.country(origin),
            origin,
            x.f("prefixes"),
        ),
        "ioda" => {
            let src = match x.s("source").as_str() {
                "bgp" => "routes in BGP",
                "ping-slash24" => "addresses answering pings",
                "merit-nt" => "traffic to a darknet",
                s => s,
            }
            .to_string();
            (x.s("id"), format!("{}: {src} fell", x.s("name")), format!("IODA, score {:.0}", x.f("score")), x.s("country"), x.i("asn"), x.f("score"))
        }
        "slow-country" => (
            x.s("country"),
            format!("{} reaches the K root server in {:.0} ms", n.place(&x.s("country")), x.f("med")),
            format!("{} of {} probes over 100 ms", x.i("slow").min(x.i("probes")), x.i("probes")),
            x.s("country"),
            0,
            x.f("med"),
        ),
        _ => return None,
    };
    let first = if r.id == "ioda" { x.s("start") } else { x.s("first") };
    let last = if r.id == "ioda" { x.s("until") } else { x.s("last") };
    Some(json!({
        "id": format!("{}:{subject}", r.id), "rule": r.id, "kind": r.kind, "severity": r.severity, "subject": subject,
        "prefix": if prefix.is_empty() { Value::Null } else { json!(prefix) },
        "origin": if origin > 0 { json!(origin) } else { Value::Null }, "owner": if owner > 0 { json!(owner) } else { Value::Null },
        "asn": if asn > 0 { json!(asn) } else { Value::Null }, "country": country, "peers": x.i("peers"), "updates": x.i("updates"),
        "value": value, "title": title, "detail": detail, "first_seen": first, "last_seen": last, "state": "open",
    }))
}

/// Writes what this run found as open, keeps the first time an alert was
/// seen, and ends the open alerts of the rules that ran and no longer find
/// them.
async fn store(db: &Strata, found: Vec<Value>, ran: &[String], now: &str) -> Result<usize> {
    let open_before = db.query("SELECT id, rule, first_seen FROM bb_alerts WHERE state = 'open' LIMIT 10000", Value::Null).await.unwrap_or_default();
    let mut first: FxHashMap<String, String> = FxHashMap::default();
    for r in &open_before {
        first.insert(r.s("id"), r.s("first_seen"));
    }
    let ids: FxHashSet<String> = found.iter().filter_map(|a| a["id"].as_str().map(str::to_string)).collect();
    let mut ops = Vec::new();
    for mut a in found {
        // A rule without times of its own (a country's pings): now.
        for k in ["first_seen", "last_seen"] {
            if a[k].as_str().is_none_or(str::is_empty) {
                a[k] = json!(now);
            }
        }
        if let Some(f) = a["id"].as_str().and_then(|id| first.get(id)) {
            if !f.is_empty() {
                a["first_seen"] = json!(f);
            }
        }
        ops.push(json!({"op": "upsert", "table": "bb_alerts", "doc": a}));
    }
    for r in &open_before {
        if !ids.contains(&r.s("id")) && ran.contains(&r.s("rule")) {
            ops.push(json!({"op": "set", "table": "bb_alerts", "key": r.s("id"), "field": "state", "value": "ended"}));
        }
    }
    let n = ids.len();
    db.write(ops).await.map_err(|e| anyhow::anyhow!("bb_alerts: {e}"))?;
    Ok(n)
}

/// Open alerts and those ended in the last hour, grouped: routes in one
/// owner's space, one announcer's, one country's outages, one prefix's
/// flapping, one country's slowness.
async fn group(db: &Strata, n: &Names) -> Result<usize> {
    let rows = db
        .query("SELECT id, rule, kind, severity, subject, prefix, origin, owner, asn, country, peers, updates, value, title, detail, first_seen, last_seen, state FROM bb_alerts WHERE state = 'open' OR last_seen >= now() - INTERVAL '6 hours' ORDER BY last_seen DESC LIMIT 5000", Value::Null)
        .await
        .unwrap_or_default();
    let mut groups: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        let key = match r.s("rule").as_str() {
            "rpki-invalid" | "new-origin" | "more-specific" => format!("owner:{}", r.i("owner")),
            "blackhole" => format!("blackhole:{}", r.i("origin")),
            "flapping" => format!("flap:{}", r.i("origin")),
            "withdrawn" => format!("outage:{}", if r.s("country").is_empty() { r.s("subject") } else { r.s("country") }),
            "ioda" => format!("outage:{}", if r.s("country").is_empty() { r.s("subject") } else { r.s("country") }),
            "slow-country" => "slow:world".to_string(),
            _ => r.s("id"),
        };
        groups.entry(key).or_default().push(r);
    }
    let mut ops = Vec::new();
    let mut ids = Vec::new();
    for (key, list) in &groups {
        let open = list.iter().filter(|r| r.s("state") == "open").count();
        let sev = if list.iter().any(|r| r.s("severity") == "bad" && r.s("state") == "open") {
            "bad"
        } else if open > 0 {
            if list.iter().any(|r| r.s("severity") == "warn") { "warn" } else { "info" }
        } else {
            "ended"
        };
        let asns: Vec<i64> = {
            let mut s: Vec<i64> = list.iter().flat_map(|r| [r.i("origin"), r.i("owner"), r.i("asn")]).filter(|a| *a > 0).collect();
            s.sort();
            s.dedup();
            s
        };
        let countries: Vec<String> = {
            let mut s: Vec<String> = list.iter().map(|r| r.s("country")).filter(|c| !c.is_empty()).collect();
            s.sort();
            s.dedup();
            s
        };
        let prefixes: Vec<String> = {
            let mut s: Vec<String> = list.iter().map(|r| r.s("prefix")).filter(|c| !c.is_empty()).collect();
            s.sort();
            s.dedup();
            s.truncate(200);
            s
        };
        let first = list.iter().map(|r| r.s("first_seen")).filter(|s| !s.is_empty()).min().unwrap_or_default();
        let last = list.iter().map(|r| r.s("last_seen")).max().unwrap_or_default();
        let kind = list[0].s("kind");
        let (title, summary) = describe(key, list, n);
        let id = format!("{}-{}", key.replace([':', '/', '|'], "-"), first.get(..10).unwrap_or(""));
        ids.push(id.clone());
        ops.push(json!({"op": "upsert", "table": "bb_incidents", "doc": {
            "id": id, "kind": kind, "severity": sev, "title": title, "summary": summary, "shared": key, "alerts": list.len(),
            "asns": asns, "countries": countries, "prefixes": prefixes, "first_seen": first, "last_seen": last,
            "state": if open > 0 { "open" } else { "ended" },
        }}));
        for r in list {
            ops.push(json!({"op": "set", "table": "bb_alerts", "key": r.s("id"), "field": "incident", "value": id}));
        }
    }
    // Open incidents this run did not make again: their alerts ended
    // long ago, or now belong to another group.
    let before = db.query("SELECT id FROM bb_incidents WHERE state = 'open' LIMIT 10000", Value::Null).await.unwrap_or_default();
    for r in &before {
        if !ids.contains(&r.s("id")) {
            ops.push(json!({"op": "set", "table": "bb_incidents", "key": r.s("id"), "field": "state", "value": "ended"}));
        }
    }
    db.write(ops).await.map_err(|e| anyhow::anyhow!("bb_incidents: {e}"))?;
    Ok(ids.len())
}

fn describe(key: &str, list: &[&Row], n: &Names) -> (String, String) {
    let open: Vec<&&Row> = list.iter().filter(|r| r.s("state") == "open").collect();
    let count = |f: &dyn Fn(&Row) -> i64| {
        let mut s: Vec<i64> = list.iter().map(|r| f(r)).filter(|x| *x > 0).collect();
        s.sort();
        s.dedup();
        s
    };
    let (kind, what) = key.split_once(':').unwrap_or((key, ""));
    match kind {
        "owner" => {
            let owner = what.parse::<i64>().unwrap_or(0);
            let origins = count(&|r: &Row| r.i("origin"));
            let names: Vec<String> = origins.iter().take(4).map(|a| n.name(*a)).collect();
            let rule = list[0].s("rule");
            let how = match rule.as_str() {
                "rpki-invalid" => "RPKI-invalid",
                "new-origin" => "with a new origin",
                _ => "inside it",
            };
            let title = if list.len() == 1 {
                list[0].s("title")
            } else {
                format!("{} routes {how} in {}'s space, announced by {}", list.len(), n.name(owner), plural(origins.len(), "network"))
            };
            (title, format!("{} still up; announced by {}", open.len(), names.join(", ")))
        }
        "outage" => {
            let ioda = list.iter().filter(|r| r.s("rule") == "ioda").count();
            let bgp = list.len() - ioda;
            let place = n.place(what);
            let names: Vec<String> = list.iter().map(|r| r.s("title").split(':').next().unwrap_or("").to_string()).filter(|t| !t.is_empty()).take(4).collect();
            (
                if list.len() == 1 { list[0].s("title") } else { format!("Outages in {place}: {}", plural(list.len(), "signal")) },
                match (ioda, bgp) {
                    (_, 0) => format!("IODA: {}", names.join(", ")),
                    (0, _) => format!("{bgp} networks' prefixes withdrawn by many of RIS's peers"),
                    _ => format!("{ioda} from IODA ({}), {bgp} from withdrawals RIS heard", names.join(", ")),
                },
            )
        }
        "flap" => {
            let origin = what.parse::<i64>().unwrap_or(0);
            let total: i64 = list.iter().map(|r| r.i("updates")).sum();
            let pfx: Vec<String> = list.iter().map(|r| r.s("prefix")).take(4).collect();
            (
                if list.len() == 1 { list[0].s("title") } else { format!("{} prefixes of {} flapping: {total} updates in an hour", list.len(), n.name(origin)) },
                if list.len() == 1 { format!("{}; {}", list[0].s("detail"), n.place(&n.country(origin))) } else { pfx.join(", ") },
            )
        }
        "slow" => {
            let mut v: Vec<&&Row> = list.iter().filter(|r| r.s("state") == "open").collect();
            v.sort_by(|a, b| b.f("value").total_cmp(&a.f("value")));
            let each: Vec<String> = v.iter().take(5).map(|r| format!("{} {:.0} ms", n.place(&r.s("country")), r.f("value"))).collect();
            (
                if v.len() == 1 { v[0].s("title") } else { format!("{} countries reach the K root server slowly", v.len()) },
                format!("{}{}", each.join(", "), if v.len() > 5 { format!(" and {} more", v.len() - 5) } else { String::new() }),
            )
        }
        _ => (list[0].s("title"), list.iter().map(|r| r.s("subject")).take(3).collect::<Vec<_>>().join(", ")),
    }
}

fn plural(n: usize, w: &str) -> String {
    if n == 1 {
        format!("1 {w}")
    } else {
        format!("{n} {w}s")
    }
}
