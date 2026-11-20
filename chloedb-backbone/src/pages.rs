//! What each page shows, each a few statements: the worklist (Now), the
//! map at each zoom (the world, a country, a network, a prefix), an
//! incident, search, the stack of a country (Diagnose), the rules, and how
//! it is built.

use std::collections::BTreeMap;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Value};

use crate::api::ApiError;
use crate::mrt::Prefix;
use crate::rules::{parse_ts, Names};
use crate::strata::{Row, RowExt, Strata, Trace};

/// The K root server's network (RIPE NCC), and RIPE Atlas's built-in
/// measurements of it from every probe.
pub const K_ROOT: i64 = 25152;
pub const PING_K: i64 = 1001;
pub const TRACE_K: i64 = 5001;

pub struct Ctx<'a> {
    pub db: &'a Strata,
    pub trace: &'a Trace,
    pub names: &'a Arc<Names>,
}

type R<T> = Result<T, ApiError>;

impl Ctx<'_> {
    pub async fn rows(&self, what: &str, sql: impl AsRef<str>, params: Value) -> R<Vec<Row>> {
        Ok(self.db.traced(self.trace, what, sql.as_ref(), params).await?)
    }

    /// As `rows`, an empty answer when ChloeDB refuses.
    pub async fn soft(&self, what: &str, sql: impl AsRef<str>, params: Value) -> Vec<Row> {
        self.db.traced(self.trace, what, sql.as_ref(), params).await.unwrap_or_default()
    }

    pub fn name(&self, a: i64) -> String {
        self.names.name(a)
    }

    pub fn net(&self, a: i64) -> Value {
        json!({"asn": a, "name": self.names.name(a), "country": self.names.country(a)})
    }

    /// The newest update's and ping's times (microseconds). Each only
    /// grows, so after the first page only rows since the last one seen
    /// are read: a few minutes of updates rather than three days of them
    /// (0.6 to 0.9 s on every page before).
    pub async fn clock(&self) -> (i64, i64) {
        static SEEN: std::sync::Mutex<(i64, i64)> = std::sync::Mutex::new((0, 0));
        let (su, sp) = *SEEN.lock().unwrap_or_else(|e| e.into_inner());
        let (u, p) = tokio::join!(
            self.soft("the newest update", "SELECT MAX(ts) AS t FROM bb_updates WHERE ts >= :since", json!({"since": iso(su)})),
            self.soft("the newest ping", "SELECT MAX(ts) AS t FROM bb_ping WHERE ts >= :since", json!({"since": iso(sp)})),
        );
        let t = |r: &[Row]| r.first().and_then(|x| parse_ts(&x.s("t"))).unwrap_or(0);
        let wall = chrono::Utc::now().timestamp_micros() + 60_000_000;
        let (u, p) = (t(&u).min(wall).max(su), t(&p).min(wall).max(sp));
        *SEEN.lock().unwrap_or_else(|e| e.into_inner()) = (u, p);
        (u, p)
    }
}

pub fn iso(us: i64) -> String {
    chrono::DateTime::from_timestamp_micros(us).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
}

const MIN: i64 = 60 * 1_000_000;

fn sev_rank(s: &str) -> i32 {
    match s {
        "bad" => 0,
        "warn" => 1,
        "info" => 2,
        _ => 3,
    }
}

fn incident_json(r: &Row) -> Value {
    json!({
        "id": r.s("id"), "kind": r.s("kind"), "severity": r.s("severity"), "title": r.s("title"), "summary": r.s("summary"),
        "shared": r.s("shared"), "alerts": r.i("alerts"), "asns": r.list("asns"), "countries": r.list("countries"),
        "prefixes": r.list("prefixes").into_iter().take(12).collect::<Vec<_>>(), "first": r.s("first_seen"), "last": r.s("last_seen"), "state": r.s("state"),
    })
}

const INCIDENT_COLS: &str = "id, kind, severity, title, summary, shared, alerts, asns, countries, prefixes, first_seen, last_seen, state";
const ALERT_COLS: &str = "id, rule, kind, severity, subject, prefix, origin, owner, asn, country, peers, updates, value, title, detail, first_seen, last_seen, state, incident";

fn alert_json(c: &Ctx<'_>, r: &Row) -> Value {
    let mut v = json!({
        "id": r.s("id"), "rule": r.s("rule"), "kind": r.s("kind"), "severity": r.s("severity"), "subject": r.s("subject"),
        "prefix": r.s("prefix"), "country": r.s("country"), "peers": r.i("peers"), "updates": r.i("updates"), "value": r.f("value"),
        "title": r.s("title"), "detail": r.s("detail"), "first": r.s("first_seen"), "last": r.s("last_seen"), "state": r.s("state"), "incident": r.s("incident"),
    });
    for k in ["origin", "owner", "asn"] {
        let a = r.i(k);
        if a > 0 {
            v[k] = c.net(a);
        }
    }
    v
}

/// How many of a country's networks with probes the map draws.
const SHOWN: usize = 20;

/// The middle of some places: the median latitude and longitude, so a few
/// far away (a building abroad, a probe on an island) do not move it.
fn middle(p: &[(f64, f64)]) -> Option<(f64, f64)> {
    if p.is_empty() {
        return None;
    }
    let mut la: Vec<f64> = p.iter().map(|x| x.0).collect();
    let mut lo: Vec<f64> = p.iter().map(|x| x.1).collect();
    la.sort_by(|a, b| a.total_cmp(b));
    lo.sort_by(|a, b| a.total_cmp(b));
    Some((la[la.len() / 2], lo[lo.len() / 2]))
}

/// The distance between two places, in degrees of a great circle (enough to
/// pick the nearest).
fn far(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let x = (la1.sin() * la2.sin() + la1.cos() * la2.cos() * (lo1 - lo2).cos()).clamp(-1.0, 1.0);
    x.acos().to_degrees()
}

/// The upstream an origin's routes went through: the AS before the origin
/// on each path (prepends folded).
fn upstream(path: &[i64]) -> Option<i64> {
    let mut p: Vec<i64> = Vec::new();
    for a in path {
        if p.last() != Some(a) {
            p.push(*a);
        }
    }
    (p.len() >= 2).then(|| p[p.len() - 2])
}

fn ints(r: &Row, k: &str) -> Vec<i64> {
    match r.get(k) {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_i64()).collect(),
        Some(Value::Number(n)) => n.as_i64().into_iter().collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------- Now

pub async fn now(c: &Ctx<'_>) -> R<Value> {
    let (now_u, now_p) = c.clock().await;
    let since = iso(now_u - 60 * MIN);
    let since_atlas = iso(now_p - 20 * MIN);
    let (inc, busiest, heard, weather, world, feed) = tokio::join!(
        c.soft("open incidents, and those ended in the last hour", format!("SELECT {INCIDENT_COLS} FROM bb_incidents WHERE state = 'open' OR last_seen >= :since ORDER BY last_seen DESC LIMIT 300"), json!({"since": since})),
        // The flapping rule counts every prefix's updates of the hour each
        // minute; its alerts are the busiest prefixes.
        c.soft("the busiest prefixes in the last hour", "SELECT prefix, updates AS n, origin FROM bb_alerts WHERE rule = 'flapping' AND state = 'open' ORDER BY updates DESC LIMIT 6", Value::Null),
        c.soft("what the collectors heard in the last five minutes", "SELECT COUNT(*) AS n, COUNT(DISTINCT peer) AS peers, COUNT(DISTINCT prefix) AS prefixes FROM bb_updates WHERE ts >= :since", json!({"since": iso(now_u - 5 * MIN)})),
        c.soft("the slowest countries to the K root server", "SELECT country, MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes FROM bb_ping WHERE ts >= :since AND msm = 1001 AND rtt IS NOT NULL GROUP BY country HAVING COUNT(DISTINCT probe) >= 5 ORDER BY med DESC LIMIT 6", json!({"since": since_atlas})),
        c.soft("the world's median to the K root server", "SELECT MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes FROM bb_ping WHERE ts >= :since AND msm = 1001 AND rtt IS NOT NULL", json!({"since": since_atlas})),
        c.soft("where each source stands", "SELECT id, kind, at, rows, note FROM bb_feed LIMIT 50", Value::Null),
    );
    let mut open: Vec<Value> = Vec::new();
    let mut ended: Vec<Value> = Vec::new();
    for r in &inc {
        if r.s("state") == "open" {
            open.push(incident_json(r));
        } else {
            ended.push(incident_json(r));
        }
    }
    open.sort_by(|a, b| {
        sev_rank(a["severity"].as_str().unwrap_or("")).cmp(&sev_rank(b["severity"].as_str().unwrap_or(""))).then(b["alerts"].as_i64().cmp(&a["alerts"].as_i64()))
    });
    let connect = connections(c, &open, &since).await;
    let h = heard.first();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for i in &open {
        *kinds.entry(i["kind"].as_str().unwrap_or("").to_string()).or_default() += 1;
    }
    let connected: FxHashSet<String> = connect.iter().flat_map(|x| x["incidents"].as_array().cloned().unwrap_or_default()).filter_map(|v| v.as_str().map(str::to_string)).collect();
    Ok(json!({
        "as_of": iso(now_u), "atlas_as_of": iso(now_p),
        "heard": {"minutes": 5, "updates": h.map(|r| r.i("n")), "peers": h.map(|r| r.i("peers")), "prefixes": h.map(|r| r.i("prefixes"))},
        "open": open, "ended": ended, "kinds": kinds, "connected": connected.len(), "connect": connect,
        "busiest": busiest.iter().map(|r| json!({"prefix": r.s("prefix"), "updates": r.i("n"), "origin": c.net(r.i("origin"))})).collect::<Vec<_>>(),
        "weather": weather.iter().map(|r| json!({"country": r.s("country"), "med": r.f("med"), "probes": r.i("probes")})).collect::<Vec<_>>(),
        "world": world.first().map(|r| json!({"med": r.f("med"), "probes": r.i("probes")})),
        "feed": feed.iter().map(|r| json!({"id": r.s("id"), "kind": r.s("kind"), "at": r.s("at"), "rows": r.i("rows"), "note": r.s("note")})).collect::<Vec<_>>(),
    }))
}

/// What open incidents share: the network just before the origin on their
/// routes (their upstream), the network that announced them, the country.
async fn connections(c: &Ctx<'_>, open: &[Value], since: &str) -> Vec<Value> {
    let mut by_up: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    let mut by_origin: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    let mut by_country: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let routing: Vec<&Value> = open.iter().filter(|i| i["prefixes"].as_array().is_some_and(|p| !p.is_empty())).take(12).collect();
    let mut prefixes: Vec<String> = Vec::new();
    let mut owner_of: FxHashMap<String, String> = FxHashMap::default();
    for i in &routing {
        for p in i["prefixes"].as_array().into_iter().flatten().take(8) {
            if let Some(p) = p.as_str() {
                prefixes.push(p.to_string());
                owner_of.insert(p.to_string(), i["id"].as_str().unwrap_or("").to_string());
            }
        }
    }
    if !prefixes.is_empty() {
        let rows = c
            .soft("the routes of the open incidents' prefixes", "SELECT prefix, origin, path FROM bb_updates WHERE prefix IN (:p) AND kind = 'A' AND ts >= :since LIMIT 4000", json!({"p": prefixes, "since": since}))
            .await;
        let mut seen: FxHashSet<(i64, String)> = FxHashSet::default();
        for r in &rows {
            let Some(inc) = owner_of.get(&r.s("prefix")) else { continue };
            if let Some(u) = upstream(&ints(r, "path")) {
                if seen.insert((u, inc.clone())) {
                    by_up.entry(u).or_default().push(inc.clone());
                }
            }
            let o = r.i("origin");
            if o > 0 && seen.insert((-o, inc.clone())) {
                by_origin.entry(o).or_default().push(inc.clone());
            }
        }
    }
    for i in open {
        for cc in i["countries"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
            by_country.entry(cc.to_string()).or_default().push(i["id"].as_str().unwrap_or("").to_string());
        }
    }
    let mut out = Vec::new();
    for (o, list) in by_origin.iter().filter(|(_, l)| l.len() >= 2) {
        out.push(json!({"how": "announcer", "title": format!("Same announcer: {}", c.name(*o)), "net": c.net(*o), "incidents": list, "text": format!("{} announced the routes of {} incidents", c.name(*o), list.len())}));
    }
    let mut ups: Vec<(&i64, &Vec<String>)> = by_up.iter().filter(|(_, l)| l.len() >= 2).collect();
    ups.sort_by_key(|(_, l)| std::cmp::Reverse(l.len()));
    for (u, list) in ups.into_iter().take(4) {
        out.push(json!({"how": "upstream", "title": format!("Same upstream: {}", c.name(*u)), "net": c.net(*u), "incidents": list, "text": format!("{} carried the routes of {} incidents to the world, the last network before their origin", c.name(*u), list.len())}));
    }
    for (cc, list) in by_country.iter().filter(|(_, l)| l.len() >= 2).take(4) {
        out.push(json!({"how": "country", "title": format!("Same country: {cc}"), "country": cc, "incidents": list, "text": format!("{} incidents involve networks in {cc}", list.len())}));
    }
    out
}

// ---------------------------------------------------------------- Map: the world

pub async fn world(c: &Ctx<'_>) -> R<Value> {
    let (now_u, now_p) = c.clock().await;
    let since_atlas = iso(now_p - 20 * MIN);
    let (alerts, pings, ixes, inc, counts) = tokio::join!(
        c.soft("open alerts", format!("SELECT {ALERT_COLS} FROM bb_alerts WHERE state = 'open' ORDER BY last_seen DESC LIMIT 3000"), Value::Null),
        c.soft("each country's probes to the K root server", "SELECT country, MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes, COUNT(DISTINCT probe) FILTER (WHERE rtt > 100) AS slow FROM bb_ping WHERE ts >= :since AND msm = 1001 AND rtt IS NOT NULL GROUP BY country LIMIT 300", json!({"since": since_atlas})),
        c.soft("the largest exchanges", "SELECT id, short, city, country, members, lat, lon FROM bb_ix WHERE lat IS NOT NULL ORDER BY members DESC LIMIT 250", Value::Null),
        c.soft("open incidents", format!("SELECT {INCIDENT_COLS} FROM bb_incidents WHERE state = 'open' ORDER BY last_seen DESC LIMIT 100"), Value::Null),
        scale_counts(c),
    );
    let mut countries: BTreeMap<String, Value> = BTreeMap::new();
    for p in &pings {
        countries.insert(p.s("country"), json!({"cc": p.s("country"), "med": p.f("med"), "probes": p.i("probes"), "slow": p.i("slow"), "alerts": 0, "severity": if p.f("med") > 150.0 { "bad" } else if p.f("med") > 100.0 { "warn" } else { "ok" }}));
    }
    let mut lines: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for a in &alerts {
        let cc = a.s("country");
        if cc.is_empty() {
            continue;
        }
        let e = countries.entry(cc.clone()).or_insert_with(|| json!({"cc": cc, "alerts": 0, "severity": "ok"}));
        e["alerts"] = json!(e["alerts"].as_i64().unwrap_or(0) + 1);
        let s = a.s("severity");
        if sev_rank(&s) < sev_rank(e["severity"].as_str().unwrap_or("ok")) {
            e["severity"] = json!(s);
        }
        // An announcer in one country taking routes of an owner in another.
        let (o, w) = (a.i("origin"), a.i("owner"));
        if o > 0 && w > 0 && o != w {
            let (co, cw) = (c.names.country(o), c.names.country(w));
            if !co.is_empty() && !cw.is_empty() && co != cw {
                let e = lines.entry((co, cw)).or_insert((0, a.s("incident")));
                e.0 += 1;
            }
        }
    }
    let red = countries.values().filter(|v| v["severity"] == "bad").count();
    Ok(json!({
        "as_of": iso(now_u), "atlas_as_of": iso(now_p),
        "countries": countries.into_values().collect::<Vec<_>>(),
        "ixes": ixes.iter().map(|r| json!({"id": r.i("id"), "name": r.s("short"), "city": r.s("city"), "cc": r.s("country"), "members": r.i("members"), "lat": r.f("lat"), "lon": r.f("lon")})).collect::<Vec<_>>(),
        "lines": lines.into_iter().map(|((a, b), (n, inc))| json!({"from": a, "to": b, "alerts": n, "incident": inc})).collect::<Vec<_>>(),
        "incidents": inc.iter().map(incident_json).collect::<Vec<_>>(),
        "alerts": alerts.iter().take(60).map(|r| alert_json(c, r)).collect::<Vec<_>>(),
        "open_alerts": alerts.len(), "red_countries": red, "counts": counts,
    }))
}

// ---------------------------------------------------------------- Map: a country

pub async fn country(c: &Ctx<'_>, cc: &str) -> R<Value> {
    let (now_u, now_p) = c.clock().await;
    let since_atlas = iso(now_p - 20 * MIN);
    let since_tr = iso(now_p - 60 * MIN);
    let p = json!({"cc": cc, "since": since_atlas});
    let (ixes, nets, probes, med, alerts, outages, abroad, ixn, facs, at) = tokio::join!(
        c.soft("the country's largest exchanges", "SELECT id, short, city, members, lat, lon FROM bb_ix WHERE country = :cc ORDER BY members DESC LIMIT 24", json!({"cc": cc})),
        c.soft("the country's networks with probes, and their time to K", "SELECT asn, COUNT(DISTINCT probe) AS probes, MEDIAN(rtt) AS med, MIN(rtt) AS best, MAX(rtt) AS worst FROM bb_ping WHERE ts >= :since AND msm = 1001 AND country = :cc AND rtt IS NOT NULL GROUP BY asn ORDER BY probes DESC LIMIT 40", p.clone()),
        c.soft("each probe's latest ping", "SELECT probe, asn, rtt, ts FROM bb_ping WHERE ts >= :since AND msm = 1001 AND country = :cc ORDER BY ts DESC LIMIT 3000", p.clone()),
        c.soft("the country's median", "SELECT MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes, COUNT(DISTINCT probe) FILTER (WHERE rtt > 100) AS slow FROM bb_ping WHERE ts >= :since AND msm = 1001 AND country = :cc AND rtt IS NOT NULL", p.clone()),
        c.soft("open alerts in the country", format!("SELECT {ALERT_COLS} FROM bb_alerts WHERE state = 'open' AND country = :cc ORDER BY last_seen DESC LIMIT 100"), json!({"cc": cc})),
        c.soft("IODA's open outages in the country", "SELECT id, kind, entity, name, source, start, score FROM bb_outage WHERE open AND country = :cc ORDER BY score DESC LIMIT 10", json!({"cc": cc})),
        c.soft("where slow traceroutes from the country jump", "SELECT jump_as, COUNT(*) AS n, MEDIAN(rtt) AS med, MEDIAN(jump_ms) AS jump FROM bb_trace WHERE ts >= :since AND msm = 5001 AND country = :cc AND rtt > 100 AND jump_as IS NOT NULL GROUP BY jump_as ORDER BY n DESC LIMIT 4", json!({"cc": cc, "since": since_tr})),
        c.soft("how many exchanges the country has", "SELECT COUNT(*) AS n FROM bb_ix WHERE country = :cc", json!({"cc": cc})),
        c.soft("the country's buildings where networks meet", "SELECT id, short, city, lat, lon, nets, ixes FROM bb_fac WHERE country = :cc AND lat IS NOT NULL ORDER BY nets DESC LIMIT 80", json!({"cc": cc})),
        c.soft("where its RIPE Atlas probes are", "SELECT id, asn, lat, lon FROM bb_probe WHERE country = :cc AND lat IS NOT NULL LIMIT 5000", json!({"cc": cc})),
    );
    let ix_ids: Vec<i64> = ixes.iter().map(|r| r.i("id")).collect();
    let mut asns: Vec<i64> = nets.iter().take(SHOWN).map(|r| r.i("asn")).filter(|a| *a > 0).collect();
    for a in &alerts {
        for k in ["origin", "asn"] {
            let x = a.i(k);
            if x > 0 && c.names.country(x) == cc && !asns.contains(&x) && asns.len() < SHOWN + 4 {
                asns.push(x);
            }
        }
    }
    let (members, k_at, all_member, sites) = tokio::join!(
        c.soft("which of these networks are on which exchange", "SELECT ix, asn FROM bb_member WHERE asn IN (:a) AND ix IN (:x) LIMIT 2000", json!({"a": asns, "x": ix_ids})),
        c.soft("where the K root server is a member", "SELECT ix FROM bb_member WHERE asn = 25152 AND ix IN (:x)", json!({"x": ix_ids})),
        c.soft("the exchanges each network is on, anywhere", "SELECT m.asn AS asn, x.country AS cc, COUNT(*) AS n FROM bb_member m JOIN bb_ix x ON m.ix = x.id WHERE m.asn IN (:a) GROUP BY m.asn, x.country LIMIT 500", json!({"a": asns})),
        c.soft("the buildings each network is in, in the country", "SELECT n.asn AS asn, f.id AS fac, f.lat AS lat, f.lon AS lon FROM bb_netfac n JOIN bb_fac f ON n.fac = f.id WHERE n.asn IN (:a) AND f.country = :cc AND f.lat IS NOT NULL LIMIT 4000", json!({"a": asns, "cc": cc})),
    );
    let mut latest: FxHashMap<i64, (i64, Option<f64>)> = FxHashMap::default();
    for r in &probes {
        latest.entry(r.i("probe")).or_insert((r.i("asn"), r.get("rtt").and_then(|v| v.as_f64())));
    }
    let mut dots: FxHashMap<i64, Vec<Value>> = FxHashMap::default();
    for (id, (asn, rtt)) in &latest {
        dots.entry(*asn).or_default().push(json!({"probe": id, "rtt": rtt}));
    }
    let mut on: FxHashMap<i64, Vec<i64>> = FxHashMap::default();
    for m in &members {
        on.entry(m.i("asn")).or_default().push(m.i("ix"));
    }
    let mut here: FxHashMap<i64, i64> = FxHashMap::default();
    let mut anywhere: FxHashMap<i64, i64> = FxHashMap::default();
    for r in &all_member {
        *anywhere.entry(r.i("asn")).or_default() += r.i("n");
        if r.s("cc") == cc {
            *here.entry(r.i("asn")).or_default() += r.i("n");
        }
    }
    let k_ix: Vec<i64> = k_at.iter().map(|r| r.i("ix")).collect();
    // Where to draw each network: the middle of its buildings in the
    // country, else of its probes there.
    let mut probe_at: FxHashMap<i64, Vec<(f64, f64)>> = FxHashMap::default();
    for r in &at {
        probe_at.entry(r.i("asn")).or_default().push((r.f("lat"), r.f("lon")));
    }
    let mut fac_at: FxHashMap<i64, Vec<(f64, f64)>> = FxHashMap::default();
    let mut fac_ids: FxHashMap<i64, Vec<i64>> = FxHashMap::default();
    for r in &sites {
        fac_at.entry(r.i("asn")).or_default().push((r.f("lat"), r.f("lon")));
        fac_ids.entry(r.i("asn")).or_default().push(r.i("fac"));
    }
    let stats: FxHashMap<i64, &Row> = nets.iter().map(|r| (r.i("asn"), r)).collect();
    let bad_nets: FxHashSet<i64> = alerts.iter().flat_map(|a| [a.i("origin"), a.i("asn")]).collect();
    let net_list: Vec<Value> = asns
        .iter()
        .map(|a| {
            let s = stats.get(a);
            let med = s.map(|r| r.f("med"));
            let mut d = dots.remove(a).unwrap_or_default();
            d.sort_by(|x, y| x["rtt"].as_f64().unwrap_or(9e9).total_cmp(&y["rtt"].as_f64().unwrap_or(9e9)));
            let state = if bad_nets.contains(a) { "bad" } else { match med { Some(m) if m > 150.0 => "bad", Some(m) if m > 60.0 => "warn", Some(_) => "ok", None => "none" } };
            let (pos, from) = match (fac_at.get(a).and_then(|p| middle(p)), probe_at.get(a).and_then(|p| middle(p))) {
                (Some(p), _) => (Some(p), "buildings"),
                (None, Some(p)) => (Some(p), "probes"),
                _ => (None, ""),
            };
            json!({
                "asn": a, "name": c.name(*a), "probes": s.map(|r| r.i("probes")).unwrap_or(0), "med": med, "best": s.map(|r| r.f("best")),
                "lat": pos.map(|p| p.0), "lon": pos.map(|p| p.1), "at": from, "facs": fac_ids.get(a).cloned().unwrap_or_default(),
                "dots": d, "ixes": on.get(a).cloned().unwrap_or_default(), "ixes_here": here.get(a).copied().unwrap_or(0),
                "ixes_anywhere": anywhere.get(a).copied().unwrap_or(0), "state": state, "at_k": on.get(a).is_some_and(|l| l.iter().any(|x| k_ix.contains(x))),
            })
        })
        .collect();
    let others = nets.len().saturating_sub(SHOWN);
    let rest: Vec<f64> = nets.iter().skip(SHOWN).map(|r| r.f("med")).collect();
    let m = med.first();
    Ok(json!({
        "cc": cc, "as_of": iso(now_u), "atlas_as_of": iso(now_p),
        "probes": m.map(|r| r.i("probes")), "med": m.map(|r| r.f("med")), "slow": m.map(|r| r.i("slow")),
        "ixes": ixes.iter().map(|r| json!({"id": r.i("id"), "name": r.s("short"), "city": r.s("city"), "members": r.i("members"), "k": k_ix.contains(&r.i("id")), "lat": r.get("lat"), "lon": r.get("lon")})).collect::<Vec<_>>(),
        "facs": facs.iter().map(|r| json!({"id": r.i("id"), "name": r.s("short"), "city": r.s("city"), "lat": r.f("lat"), "lon": r.f("lon"), "nets": r.i("nets"), "ixes": r.i("ixes")})).collect::<Vec<_>>(),
        "probes_at": at.iter().map(|r| json!([r.i("id"), r.i("asn"), r.f("lat"), r.f("lon"), latest.get(&r.i("id")).and_then(|x| x.1).map(|v| v.round())])).collect::<Vec<_>>(),
        "ix_total": ixn.first().map(|r| r.i("n")).unwrap_or(0),
        "nets": net_list,
        "more": {"networks": others, "under20": rest.iter().filter(|x| **x < 20.0).count(), "over90": rest.iter().filter(|x| **x > 90.0).count()},
        "abroad": abroad.iter().map(|r| json!({"net": c.net(r.i("jump_as")), "traces": r.i("n"), "med": r.f("med"), "jump": r.f("jump")})).collect::<Vec<_>>(),
        "alerts": alerts.iter().map(|r| alert_json(c, r)).collect::<Vec<_>>(),
        "outages": outages.iter().map(|r| json!({"id": r.s("id"), "kind": r.s("kind"), "name": r.s("name"), "source": r.s("source"), "start": r.s("start"), "score": r.f("score")})).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------- Map: a network

pub async fn network(c: &Ctx<'_>, asn: i64) -> R<Value> {
    let (now_u, now_p) = c.clock().await;
    let a = json!({"a": asn});
    let since_day = iso(now_u - 24 * 60 * MIN);
    let since3h = iso(now_u - 180 * MIN);
    let since_atlas = iso(now_p - 20 * MIN);
    let row = c.rows("the network", "SELECT asn, short, name, handle, country, kind, scope, traffic, website, pdb, prefixes4, prefixes6, ixes, facs, providers, customers, peers, roas, probes, updated, created FROM bb_as WHERE asn = :a", a.clone()).await?;
    let Some(n) = row.first() else {
        return Ok(json!({"found": false, "asn": asn}));
    };
    let big = n.i("customers") > 1500;
    let (providers, customers, peers, origin, rpki, alerts, ixes, roas, minutes, pings, facs) = tokio::join!(
        c.soft("its providers: one hop in along bb_p2c", "SELECT asn, short, country, ixes, customers, providers FROM bb_as WHERE REACHABLE(bb_p2c, :a, dir => 'in') ORDER BY customers DESC LIMIT 20", a.clone()),
        c.soft("its customers: one hop out along bb_p2c", "SELECT asn, short, country, customers FROM bb_as WHERE REACHABLE(bb_p2c, :a, dir => 'out') ORDER BY customers DESC LIMIT 12", a.clone()),
        c.soft("how many peers it has", "SELECT COUNT(*) AS n FROM bb_as WHERE REACHABLE(bb_p2p, :a, dir => 'both')", a.clone()),
        c.soft("the prefixes it announced in the last day", "SELECT prefix, COUNT(*) AS n, COUNT(DISTINCT peer) AS peers, MAX(ts) AS last FROM bb_updates WHERE origin = :a AND ts >= :since GROUP BY prefix ORDER BY n DESC LIMIT 60", json!({"a": asn, "since": since_day})),
        c.soft("its announcements by RPKI state", "SELECT rpki, COUNT(*) AS n, COUNT(DISTINCT prefix) AS prefixes FROM bb_updates WHERE origin = :a AND kind = 'A' AND ts >= :since GROUP BY rpki", json!({"a": asn, "since": since_day})),
        c.soft("alerts about it", format!("SELECT {ALERT_COLS} FROM bb_alerts WHERE origin = :a OR owner = :a OR asn = :a ORDER BY last_seen DESC LIMIT 40"), a.clone()),
        c.soft("the exchanges it is on", "SELECT m.ix AS ix, x.short AS name, x.city AS city, x.country AS cc, x.lat AS lat, x.lon AS lon, m.speed AS speed, m.created AS created, m.updated AS updated FROM bb_member m JOIN bb_ix x ON m.ix = x.id WHERE m.asn = :a ORDER BY m.speed DESC LIMIT 60", a.clone()),
        c.soft("its ROAs", "SELECT prefix, maxlen, plen, ta FROM bb_roa WHERE asn = :a ORDER BY plen LIMIT 300", a.clone()),
        c.soft("its prefixes' updates a minute, last three hours", "SELECT date_trunc('minute', ts) AS m, COUNT(*) AS n, COUNT(*) FILTER (WHERE kind = 'W') AS w FROM bb_updates WHERE origin = :a AND ts >= :since GROUP BY m ORDER BY m LIMIT 200", json!({"a": asn, "since": since3h})),
        c.soft("its probes' latest pings to the K root server", "SELECT probe, rtt, country, ts FROM bb_ping WHERE asn = :a AND msm = 1001 AND ts >= :since ORDER BY ts DESC LIMIT 1000", json!({"a": asn, "since": since_atlas})),
        c.soft("the buildings it is in", "SELECT f.id AS id, f.short AS name, f.city AS city, f.country AS cc, f.lat AS lat, f.lon AS lon FROM bb_netfac n JOIN bb_fac f ON n.fac = f.id WHERE n.asn = :a LIMIT 500", a.clone()),
    );
    let probe_at = c.soft("where its RIPE Atlas probes are", "SELECT id, lat, lon, country FROM bb_probe WHERE asn = :a AND lat IS NOT NULL LIMIT 3000", a.clone()).await;
    // Where it is: the middle of its buildings at home, else anywhere, else
    // of its probes.
    let home = n.s("country");
    let pts = |rs: &[Row], cc: Option<&str>| -> Vec<(f64, f64)> {
        rs.iter().filter(|r| r.get("lat").is_some_and(|v| v.is_number()) && cc.is_none_or(|cc| r.s("cc") == cc || r.s("country") == cc)).map(|r| (r.f("lat"), r.f("lon"))).collect()
    };
    let centre = middle(&pts(&facs, Some(&home))).or_else(|| middle(&pts(&facs, None))).or_else(|| middle(&pts(&probe_at, None)));
    // Where each provider meets it: a building they share, nearest its
    // middle, else the provider's building nearest it.
    let pa: Vec<i64> = providers.iter().take(8).map(|r| r.i("asn")).collect();
    let psites = if pa.is_empty() || centre.is_none() {
        Vec::new()
    } else {
        c.soft("its providers' buildings, to draw where they meet it", "SELECT n.asn AS asn, f.id AS fac, f.lat AS lat, f.lon AS lon FROM bb_netfac n JOIN bb_fac f ON n.fac = f.id WHERE n.asn IN (:p) AND f.lat IS NOT NULL LIMIT 10000", json!({"p": pa})).await
    };
    let mine: FxHashSet<i64> = facs.iter().map(|r| r.i("id")).collect();
    let mut meet: FxHashMap<i64, (bool, f64, f64, f64)> = FxHashMap::default();
    if let Some(cen) = centre {
        for r in &psites {
            let (shared, p) = (mine.contains(&r.i("fac")), (r.f("lat"), r.f("lon")));
            let d = far(cen, p);
            let e = meet.entry(r.i("asn")).or_insert((shared, d, p.0, p.1));
            if (shared && !e.0) || (shared == e.0 && d < e.1) {
                *e = (shared, d, p.0, p.1);
            }
        }
    }
    // Trouble within three hops: open alerts' networks near it.
    let bad = c.soft("networks with open alerts", "SELECT origin, owner, asn, severity, title FROM bb_alerts WHERE state = 'open' LIMIT 3000", Value::Null).await;
    let mut bad_set: Vec<i64> = bad.iter().flat_map(|r| [r.i("origin"), r.i("owner"), r.i("asn")]).filter(|x| *x > 0 && *x != asn).collect();
    bad_set.sort();
    bad_set.dedup();
    let hops = if big { 1 } else { 3 };
    let near = if bad_set.is_empty() {
        Vec::new()
    } else {
        c.soft(
            &format!("trouble within {hops} hops: REACHABLE along bb_p2c both ways, among networks with open alerts"),
            &format!("SELECT asn, short, country, hops() AS h FROM bb_as WHERE REACHABLE(bb_p2c, :a, hops => {hops}, dir => 'both', max_nodes => 200000) AND asn IN (:bad) ORDER BY hops() LIMIT 10"),
            json!({"a": asn, "bad": bad_set}),
        )
        .await
    };
    let bad_title: FxHashMap<i64, (String, String)> = bad.iter().flat_map(|r| [r.i("origin"), r.i("owner"), r.i("asn")].into_iter().map(move |x| (x, (r.s("severity"), r.s("title"))))).collect();
    let mut latest: FxHashMap<i64, (Option<f64>, String)> = FxHashMap::default();
    for r in &pings {
        latest.entry(r.i("probe")).or_insert((r.get("rtt").and_then(|v| v.as_f64()), r.s("country")));
    }
    let probes_at: Vec<Value> = probe_at.iter().map(|r| json!([r.i("id"), r.f("lat"), r.f("lon"), latest.get(&r.i("id")).and_then(|x| x.0).map(|v| v.round())])).collect();
    let mut rtts: Vec<f64> = latest.values().filter_map(|x| x.0).collect();
    rtts.sort_by(|a, b| a.total_cmp(b));
    let med = (!rtts.is_empty()).then(|| rtts[rtts.len() / 2]);
    let open_here: Vec<&Row> = alerts.iter().filter(|r| r.s("state") == "open").collect();
    let state = if open_here.iter().any(|r| r.s("severity") == "bad") {
        "bad"
    } else if !open_here.is_empty() || med.is_some_and(|m| m > 150.0) {
        "warn"
    } else {
        "ok"
    };
    // Changes and faults, newest first.
    let mut changes: Vec<Value> = Vec::new();
    for r in alerts.iter().take(12) {
        changes.push(json!({"at": r.s("first_seen"), "kind": "fault", "text": r.s("title"), "state": r.s("state"), "alert": r.s("id"), "incident": r.s("incident")}));
    }
    if !n.s("updated").is_empty() {
        changes.push(json!({"at": n.s("updated"), "kind": "change", "text": "Its PeeringDB record was updated"}));
    }
    for x in &ixes {
        let (cr, up) = (x.s("created"), x.s("updated"));
        if !cr.is_empty() {
            changes.push(json!({"at": cr, "kind": "change", "text": format!("Joined {} ({})", x.s("name"), x.s("city"))}));
        }
        if !up.is_empty() && up != cr {
            changes.push(json!({"at": up, "kind": "change", "text": format!("Its port at {} was updated", x.s("name"))}));
        }
    }
    changes.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    changes.truncate(14);
    let pfx: Vec<Value> = origin.iter().map(|r| json!({"prefix": r.s("prefix"), "updates": r.i("n"), "peers": r.i("peers"), "last": r.s("last")})).collect();
    Ok(json!({
        "found": true, "asn": asn, "as_of": iso(now_u),
        "net": {"asn": asn, "name": n.s("short"), "full": n.s("name"), "handle": n.s("handle"), "country": n.s("country"), "kind": n.s("kind"), "scope": n.s("scope"),
                "traffic": n.s("traffic"), "website": n.s("website"), "pdb": n.get("pdb"), "prefixes4": n.get("prefixes4"), "ixes": n.i("ixes"), "facs": facs.len(),
                "providers": n.i("providers"), "customers": n.i("customers"), "peers": peers.first().map(|r| r.i("n")).unwrap_or(n.i("peers")), "roas": n.i("roas"),
                "probes": n.i("probes"), "updated": n.s("updated")},
        "state": state, "med": med, "probes_now": latest.len(),
        "providers": providers.iter().map(|r| {
            let m = meet.get(&r.i("asn"));
            json!({"asn": r.i("asn"), "name": r.s("short"), "country": r.s("country"), "ixes": r.i("ixes"), "customers": r.i("customers"), "state": bad_title.get(&r.i("asn")).map(|x| x.0.clone()).unwrap_or("ok".into()),
                   "meet": m.map(|m| json!({"shared": m.0, "lat": m.2, "lon": m.3}))})
        }).collect::<Vec<_>>(),
        "centre": centre.map(|p| json!({"lat": p.0, "lon": p.1})),
        "sites": facs.iter().filter(|r| r.get("lat").is_some_and(|v| v.is_number())).map(|r| json!({"id": r.i("id"), "name": r.s("name"), "city": r.s("city"), "cc": r.s("cc"), "lat": r.f("lat"), "lon": r.f("lon")})).collect::<Vec<_>>(),
        "probes_at": probes_at,
        "customers": customers.iter().map(|r| json!({"asn": r.i("asn"), "name": r.s("short"), "country": r.s("country"), "customers": r.i("customers")})).collect::<Vec<_>>(),
        "trouble": near.iter().map(|r| json!({"asn": r.i("asn"), "name": r.s("short"), "country": r.s("country"), "hops": r.i("h"), "severity": bad_title.get(&r.i("asn")).map(|x| x.0.clone()), "title": bad_title.get(&r.i("asn")).map(|x| x.1.clone())})).collect::<Vec<_>>(),
        "trouble_hops": hops,
        "rpki": rpki.iter().map(|r| json!({"rpki": r.s("rpki"), "updates": r.i("n"), "prefixes": r.i("prefixes")})).collect::<Vec<_>>(),
        "prefixes": pfx,
        "roas": roas.iter().map(|r| json!({"prefix": r.s("prefix"), "maxlen": r.i("maxlen"), "ta": r.s("ta")})).collect::<Vec<_>>(),
        "alerts": alerts.iter().map(|r| alert_json(c, r)).collect::<Vec<_>>(),
        "ixes": ixes.iter().map(|r| json!({"id": r.i("ix"), "name": r.s("name"), "city": r.s("city"), "cc": r.s("cc"), "speed": r.i("speed"), "lat": r.get("lat"), "lon": r.get("lon")})).collect::<Vec<_>>(),
        "minutes": minutes.iter().map(|r| json!({"t": r.s("m"), "n": r.i("n"), "w": r.i("w")})).collect::<Vec<_>>(),
        "changes": changes,
    }))
}

// ---------------------------------------------------------------- Map: a prefix

pub async fn prefix(c: &Ctx<'_>, p: &Prefix) -> R<Value> {
    let (now_u, _) = c.clock().await;
    let ps = p.to_string();
    let covering: Vec<String> = (8..=p.len).rev().map(|l| Prefix { v6: p.v6, addr: p.addr, len: l }.masked().to_string()).collect();
    let (lo, hi) = p.range();
    let since_day = iso(now_u - 24 * 60 * MIN);
    const UPD: &str = "SELECT ts, peer, peer_as, kind, origin, path, rpki, seen, owner FROM bb_updates";
    let (rows, roas, outer, inner, alerts) = tokio::join!(
        c.soft("the prefix's newest 10,000 updates (clustered by prefix: one run of each segment)", format!("{UPD} WHERE prefix = :p ORDER BY ts DESC LIMIT 10000"), json!({"p": ps})),
        c.soft("the ROAs covering it: the prefix at each shorter length", "SELECT prefix, asn, maxlen, ta, expires FROM bb_roa WHERE prefix IN (:c) ORDER BY plen DESC LIMIT 30", json!({"c": covering})),
        c.soft("the routes covering it, in the last day", "SELECT prefix, origin, COUNT(*) AS n, COUNT(DISTINCT peer) AS peers FROM bb_updates WHERE prefix IN (:c) AND prefix != :p AND kind = 'A' AND ts >= :since GROUP BY prefix, origin ORDER BY n DESC LIMIT 20", json!({"c": covering, "p": ps, "since": since_day})),
        c.soft("the routes inside it, in the last day: lo and hi within its range", "SELECT prefix, origin, COUNT(*) AS n, COUNT(DISTINCT peer) AS peers FROM bb_updates WHERE af = :af AND lo >= :lo AND hi <= :hi AND plen > :len AND ts >= :since GROUP BY prefix, origin ORDER BY n DESC LIMIT 20", json!({"af": if p.v6 { 6 } else { 4 }, "lo": lo, "hi": hi, "len": p.len, "since": since_day})),
        c.soft("alerts about it", format!("SELECT {ALERT_COLS} FROM bb_alerts WHERE prefix = :p ORDER BY last_seen DESC LIMIT 20"), json!({"p": ps})),
    );
    // The newest 10,000 updates only. Paging further back (`ts < :before`)
    // ran 11 to 41 s on the live 264 M-row bb_updates and then timed out:
    // the range on ts is not narrowed to the prefix's rows.
    // The routing table of the prefix, replayed: each peer's last word.
    let mut ups: Vec<(i64, &Row)> = rows.iter().filter_map(|r| parse_ts(&r.s("ts")).map(|t| (t, r))).collect();
    ups.sort_by_key(|x| x.0);
    let mut held: FxHashMap<String, i64> = FxHashMap::default();
    let mut minutes: BTreeMap<i64, (i64, i64, usize)> = BTreeMap::new();
    let mut origins: BTreeMap<i64, (FxHashSet<String>, i64, i64, String, String, i64)> = BTreeMap::new();
    let mut events: Vec<Value> = Vec::new();
    let mut prev_origins: Vec<i64> = Vec::new();
    // Each peer's path to it, now and an hour ago (when the updates kept
    // reach back that far).
    let cut = now_u - 60 * MIN;
    let mut paths: FxHashMap<String, Vec<i64>> = FxHashMap::default();
    let mut before: Option<FxHashMap<String, Vec<i64>>> = None;
    let known = ups.first().is_some_and(|x| x.0 < cut);
    for (t, r) in &ups {
        if before.is_none() && *t >= cut {
            before = Some(paths.clone());
        }
        let peer = r.s("peer");
        if r.s("kind") == "A" {
            let mut p = ints(r, "path");
            p.dedup();
            paths.insert(peer.clone(), p);
        } else {
            paths.remove(&peer);
        }
        let m = t / MIN * MIN;
        let e = minutes.entry(m).or_insert((0, 0, 0));
        if r.s("kind") == "A" {
            e.0 += 1;
            let o = r.i("origin");
            held.insert(peer.clone(), o);
            let x = origins.entry(o).or_insert_with(|| (FxHashSet::default(), *t, *t, r.s("rpki"), r.s("seen"), r.i("owner")));
            x.0.insert(peer);
            x.2 = *t;
        } else {
            e.1 += 1;
            held.remove(&peer);
        }
        e.2 = held.len();
        let mut now_o: Vec<i64> = held.values().copied().collect();
        now_o.sort();
        now_o.dedup();
        if now_o != prev_origins {
            let added: Vec<i64> = now_o.iter().filter(|o| !prev_origins.contains(o)).copied().collect();
            let gone: Vec<i64> = prev_origins.iter().filter(|o| !now_o.contains(o)).copied().collect();
            for o in added {
                events.push(json!({"at": iso(*t), "kind": "change", "text": format!("{} began to announce it", c.name(o)), "origin": o}));
            }
            for o in gone {
                events.push(json!({"at": iso(*t), "kind": "change", "text": format!("The last peer dropped {}'s route", c.name(o)), "origin": o}));
            }
            prev_origins = now_o;
        }
    }
    let mut holding: BTreeMap<i64, usize> = BTreeMap::new();
    for o in held.values() {
        *holding.entry(*o).or_default() += 1;
    }
    let before = if known { Some(before.unwrap_or_else(|| paths.clone())) } else { None };
    let tally = |m: &FxHashMap<String, Vec<i64>>| -> Vec<(Vec<i64>, usize)> {
        let mut n: BTreeMap<Vec<i64>, usize> = BTreeMap::new();
        for p in m.values() {
            *n.entry(p.clone()).or_default() += 1;
        }
        let mut v: Vec<(Vec<i64>, usize)> = n.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        v
    };
    let now_paths = tally(&paths);
    let then_paths = before.as_ref().map(&tally);
    let path_json = |p: &[i64], n: usize, mark: &str| json!({"path": p.iter().map(|a| c.net(*a)).collect::<Vec<_>>(), "peers": n, "mark": mark});
    let then_set: FxHashSet<&Vec<i64>> = then_paths.iter().flatten().map(|x| &x.0).collect();
    let now_set: FxHashSet<&Vec<i64>> = now_paths.iter().map(|x| &x.0).collect();
    let paths_json = json!({
        "known": known,
        "now": now_paths.iter().take(8).map(|(p, n)| path_json(p, *n, if known && !then_set.contains(p) { "new" } else { "" })).collect::<Vec<_>>(),
        "now_total": now_paths.len(),
        "gone": then_paths.iter().flatten().filter(|(p, _)| !now_set.contains(p)).take(6).map(|(p, n)| path_json(p, *n, "gone")).collect::<Vec<_>>(),
        "then_total": then_paths.as_ref().map(|v| v.len()),
    });
    let peers_heard: FxHashSet<String> = rows.iter().map(|r| r.s("peer")).collect();
    let last_path = rows.iter().find(|r| r.s("kind") == "A").map(|r| ints(r, "path")).unwrap_or_default();
    let mut ups_by: BTreeMap<i64, usize> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.s("kind") == "A") {
        if let Some(u) = upstream(&ints(r, "path")) {
            *ups_by.entry(u).or_default() += 1;
        }
    }
    let mut upstreams: Vec<(i64, usize)> = ups_by.into_iter().collect();
    upstreams.sort_by_key(|x| std::cmp::Reverse(x.1));
    events.reverse();
    events.truncate(30);
    for a in &alerts {
        events.push(json!({"at": a.s("first_seen"), "kind": "fault", "text": a.s("title"), "state": a.s("state"), "incident": a.s("incident")}));
    }
    events.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    let owner = rows.first().map(|r| r.i("owner")).filter(|o| *o > 0).or_else(|| roas.first().map(|r| r.i("asn")));
    let state = if alerts.iter().any(|r| r.s("state") == "open" && r.s("severity") == "bad") {
        "bad"
    } else if alerts.iter().any(|r| r.s("state") == "open") {
        "warn"
    } else if alerts.is_empty() {
        "ok"
    } else {
        "ended"
    };
    Ok(json!({
        "prefix": ps, "as_of": iso(now_u), "state": state, "owner": owner.map(|o| c.net(o)),
        "updates": rows.len(), "capped": rows.len() >= 30000, "peers_heard": peers_heard.len(), "held_now": held.len(),
        "holding": holding.into_iter().map(|(o, n)| json!({"origin": c.net(o), "peers": n})).collect::<Vec<_>>(),
        "origins": origins.into_iter().map(|(o, x)| json!({"origin": c.net(o), "peers": x.0.len(), "first": iso(x.1), "last": iso(x.2), "rpki": x.3, "seen": x.4})).collect::<Vec<_>>(),
        "path": last_path.iter().map(|a| c.net(*a)).collect::<Vec<_>>(),
        "upstreams": upstreams.iter().take(8).map(|(a, n)| json!({"net": c.net(*a), "routes": n})).collect::<Vec<_>>(),
        "roas": roas.iter().map(|r| json!({"prefix": r.s("prefix"), "asn": r.i("asn"), "name": c.name(r.i("asn")), "maxlen": r.i("maxlen"), "ta": r.s("ta"), "expires": r.s("expires")})).collect::<Vec<_>>(),
        "covering": outer.iter().map(|r| json!({"prefix": r.s("prefix"), "origin": c.net(r.i("origin")), "updates": r.i("n"), "peers": r.i("peers")})).collect::<Vec<_>>(),
        "inside": inner.iter().map(|r| json!({"prefix": r.s("prefix"), "origin": c.net(r.i("origin")), "updates": r.i("n"), "peers": r.i("peers")})).collect::<Vec<_>>(),
        "minutes": minutes.into_iter().rev().take(240).rev().map(|(t, (a, w, h))| json!({"t": iso(t), "a": a, "w": w, "held": h})).collect::<Vec<_>>(),
        "events": events, "alerts": alerts.iter().map(|r| alert_json(c, r)).collect::<Vec<_>>(),
        "paths": paths_json,
    }))
}

// ---------------------------------------------------------------- Timeline

/// The strip over the console: everything heard a minute for six hours,
/// from the rollup, with the minutes it has not folded in yet read from the
/// updates, and the incidents of those hours.
pub async fn timeline(c: &Ctx<'_>) -> R<Value> {
    let (now_u, _) = c.clock().await;
    let since = iso(now_u - 360 * MIN);
    let (roll, inc) = tokio::join!(
        c.soft("updates a minute by kind and RPKI state, last six hours, from the rollup bb_updates_1m", "SELECT bucket, kind, rpki, SUM(count) AS n FROM bb_updates_1m WHERE bucket >= :since GROUP BY bucket, kind, rpki ORDER BY bucket LIMIT 9000", json!({"since": since})),
        c.soft("incidents of the last six hours", format!("SELECT {INCIDENT_COLS} FROM bb_incidents WHERE last_seen >= :since OR state = 'open' ORDER BY first_seen LIMIT 400"), json!({"since": since})),
    );
    // minute -> (announcements, withdrawals, invalid announcements)
    let mut m: BTreeMap<i64, (i64, i64, i64)> = BTreeMap::new();
    for r in &roll {
        let Some(t) = parse_ts(&r.s("bucket")) else { continue };
        let e = m.entry(t).or_default();
        let n = r.i("n");
        if r.s("kind") == "W" {
            e.1 += n;
        } else {
            e.0 += n;
            if r.s("rpki").starts_with("invalid") {
                e.2 += n;
            }
        }
    }
    // The rollup folds a minute in once it is ten minutes old: the newest
    // minutes come from the updates themselves.
    let last = m.keys().next_back().copied().unwrap_or(now_u - 360 * MIN);
    let from = (last + MIN).max(now_u - 20 * MIN);
    let fresh = c
        .soft("updates a minute since the rollup's newest minute, from bb_updates", "SELECT date_trunc('minute', ts) AS b, kind, COUNT(*) AS n, COUNT(*) FILTER (WHERE rpki IN ('invalid', 'invalid-length')) AS bad FROM bb_updates WHERE ts >= :from GROUP BY b, kind LIMIT 200", json!({"from": iso(from)}))
        .await;
    for r in &fresh {
        let Some(t) = parse_ts(&r.s("b")) else { continue };
        let e = m.entry(t).or_default();
        if r.s("kind") == "W" {
            e.1 = r.i("n");
        } else {
            e.0 = r.i("n");
            e.2 = r.i("bad");
        }
    }
    let newest = now_u / MIN * MIN;
    Ok(json!({
        "as_of": iso(now_u), "from": since, "folded_to": iso(last),
        // The minute still being heard is partial: left out.
        "minutes": m.iter().filter(|(t, _)| **t < newest).map(|(t, (a, w, bad))| json!([iso(*t), a, w, bad])).collect::<Vec<_>>(),
        "incidents": inc.iter().map(incident_json).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------- Watch

/// The networks a reader watches: for each, what is open about it, its
/// routes' updates in the last hour, and its probes' time to K.
pub async fn watch(c: &Ctx<'_>, asns: &[i64]) -> R<Value> {
    let (now_u, now_p) = c.clock().await;
    let a = json!({"a": asns});
    let (recs, alerts, upd, pings) = tokio::join!(
        c.soft("the networks watched", "SELECT asn, short, country, roas, customers FROM bb_as WHERE asn IN (:a)", a.clone()),
        c.soft("open alerts about them", "SELECT origin, owner, asn, severity, title, incident FROM bb_alerts WHERE state = 'open' AND (origin IN (:a) OR owner IN (:a) OR asn IN (:a)) LIMIT 1000", a.clone()),
        c.soft("their routes' updates in the last hour", "SELECT origin, COUNT(*) AS n, COUNT(*) FILTER (WHERE kind = 'W') AS w, COUNT(DISTINCT prefix) AS prefixes FROM bb_updates WHERE origin IN (:a) AND ts >= :since GROUP BY origin LIMIT 100", json!({"a": asns, "since": iso(now_u - 60 * MIN)})),
        c.soft("their probes' time to the K root server", "SELECT asn, MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes FROM bb_ping WHERE asn IN (:a) AND msm = 1001 AND ts >= :since AND rtt IS NOT NULL GROUP BY asn LIMIT 100", json!({"a": asns, "since": iso(now_p - 20 * MIN)})),
    );
    let rec: FxHashMap<i64, &Row> = recs.iter().map(|r| (r.i("asn"), r)).collect();
    let up: FxHashMap<i64, &Row> = upd.iter().map(|r| (r.i("origin"), r)).collect();
    let pg: FxHashMap<i64, &Row> = pings.iter().map(|r| (r.i("asn"), r)).collect();
    let list: Vec<Value> = asns
        .iter()
        .map(|x| {
            let mine: Vec<&Row> = alerts.iter().filter(|r| [r.i("origin"), r.i("owner"), r.i("asn")].contains(x)).collect();
            let worst = mine.iter().min_by_key(|r| sev_rank(&r.s("severity")));
            let med = pg.get(x).map(|r| r.f("med"));
            let state = match worst.map(|r| r.s("severity")) {
                Some(s) if s == "bad" => "bad",
                Some(_) => "warn",
                None if med.is_some_and(|m| m > 150.0) => "warn",
                None => "ok",
            };
            json!({
                "asn": x, "name": rec.get(x).map(|r| r.s("short")).filter(|s| !s.is_empty()).unwrap_or_else(|| c.name(*x)),
                "country": rec.get(x).map(|r| r.s("country")).unwrap_or_default(), "found": rec.contains_key(x),
                "state": state, "alerts": mine.len(), "title": worst.map(|r| r.s("title")), "incident": worst.map(|r| r.s("incident")),
                "updates": up.get(x).map(|r| r.i("n")).unwrap_or(0), "withdrawals": up.get(x).map(|r| r.i("w")).unwrap_or(0),
                "prefixes": up.get(x).map(|r| r.i("prefixes")).unwrap_or(0), "med": med, "probes": pg.get(x).map(|r| r.i("probes")).unwrap_or(0),
                "roas": rec.get(x).map(|r| r.i("roas")).unwrap_or(0),
            })
        })
        .collect();
    Ok(json!({"as_of": iso(now_u), "networks": list}))
}

// ---------------------------------------------------------------- Incidents

pub async fn incidents(c: &Ctx<'_>) -> R<Value> {
    let rows = c.soft("incidents of the last day", format!("SELECT {INCIDENT_COLS} FROM bb_incidents WHERE last_seen >= now() - INTERVAL '1 day' OR state = 'open' ORDER BY last_seen DESC LIMIT 300"), Value::Null).await;
    let mut list: Vec<Value> = rows.iter().map(incident_json).collect();
    list.sort_by(|a, b| {
        let open = |v: &Value| if v["state"] == "open" { 0 } else { 1 };
        open(a).cmp(&open(b)).then(sev_rank(a["severity"].as_str().unwrap_or("")).cmp(&sev_rank(b["severity"].as_str().unwrap_or("")))).then(b["last"].as_str().cmp(&a["last"].as_str()))
    });
    Ok(json!({"incidents": list}))
}

pub async fn incident(c: &Ctx<'_>, id: &str) -> R<Value> {
    let inc = c.rows("the incident", format!("SELECT {INCIDENT_COLS} FROM bb_incidents WHERE id = :id"), json!({"id": id})).await?;
    let Some(i) = inc.first() else {
        return Ok(json!({"found": false, "id": id}));
    };
    let alerts = c.soft("its alerts", format!("SELECT {ALERT_COLS} FROM bb_alerts WHERE incident = :id ORDER BY peers DESC LIMIT 200"), json!({"id": id})).await;
    let prefixes: Vec<String> = alerts.iter().map(|r| r.s("prefix")).filter(|p| !p.is_empty()).take(60).collect();
    let first = parse_ts(&i.s("first_seen")).unwrap_or(0);
    let last = parse_ts(&i.s("last_seen")).unwrap_or(0);
    let (from, to) = (iso(first - 30 * MIN), iso(last + 30 * MIN));
    let (minutes, routes) = if prefixes.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        tokio::join!(
            c.soft("its prefixes' updates a minute", "SELECT date_trunc('minute', ts) AS m, COUNT(*) FILTER (WHERE kind = 'A') AS a, COUNT(*) FILTER (WHERE kind = 'W') AS w, COUNT(DISTINCT peer) AS peers FROM bb_updates WHERE prefix IN (:p) AND ts >= :from AND ts <= :to GROUP BY m ORDER BY m LIMIT 400", json!({"p": prefixes, "from": from, "to": to})),
            c.soft("the routes its prefixes took", "SELECT prefix, origin, path, peer_as FROM bb_updates WHERE prefix IN (:p) AND kind = 'A' AND ts >= :from AND ts <= :to LIMIT 5000", json!({"p": prefixes, "from": from, "to": to})),
        )
    };
    // The graph: owners, announcers, upstreams and the peers that heard it.
    let mut nodes: BTreeMap<i64, (String, usize)> = BTreeMap::new();
    let mut edges: BTreeMap<(i64, i64, &'static str), usize> = BTreeMap::new();
    for a in &alerts {
        let (o, w) = (a.i("origin"), a.i("owner"));
        if w > 0 {
            nodes.entry(w).or_insert(("owner".into(), 0)).1 += 1;
        }
        if o > 0 {
            nodes.entry(o).or_insert(("announcer".into(), 0)).1 += 1;
        }
        if o > 0 && w > 0 && o != w {
            *edges.entry((o, w, "took")).or_default() += 1;
        }
    }
    let mut carried: BTreeMap<i64, FxHashSet<String>> = BTreeMap::new();
    for r in &routes {
        let path = ints(r, "path");
        if let Some(u) = upstream(&path) {
            let o = r.i("origin");
            carried.entry(u).or_default().insert(r.s("prefix"));
            *edges.entry((u, o, "carried")).or_default() += 1;
        }
    }
    let mut top_up: Vec<(i64, usize)> = carried.iter().map(|(a, s)| (*a, s.len())).collect();
    top_up.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (u, n) in top_up.iter().take(8) {
        nodes.entry(*u).or_insert(("upstream".into(), *n));
    }
    let keep: FxHashSet<i64> = nodes.keys().copied().collect();
    let edges: Vec<Value> = edges.into_iter().filter(|((a, b, _), _)| keep.contains(a) && keep.contains(b)).map(|((a, b, k), n)| json!({"from": a, "to": b, "kind": k, "n": n})).collect();
    Ok(json!({
        "found": true, "incident": incident_json(i),
        "alerts": alerts.iter().map(|r| alert_json(c, r)).collect::<Vec<_>>(),
        "minutes": minutes.iter().map(|r| json!({"t": r.s("m"), "a": r.i("a"), "w": r.i("w"), "peers": r.i("peers")})).collect::<Vec<_>>(),
        "graph": {"nodes": nodes.into_iter().map(|(a, (role, n))| json!({"asn": a, "name": c.name(a), "country": c.names.country(a), "role": role, "n": n})).collect::<Vec<_>>(), "edges": edges},
        "upstreams": top_up.iter().take(8).map(|(a, n)| json!({"net": c.net(*a), "prefixes": n})).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------- Search

pub async fn search(c: &Ctx<'_>, q: &str) -> R<Value> {
    let t = q.trim();
    if let Some(p) = Prefix::parse(t).or_else(|| if t.contains(':') { Prefix::parse(&format!("{t}/128")) } else { Prefix::parse(&format!("{t}/32")) }) {
        // An address or a prefix: the most specific route covering it.
        let covering: Vec<String> = (8..=p.len).rev().map(|l| Prefix { v6: p.v6, addr: p.addr, len: l }.masked().to_string()).collect();
        let rows = c.soft("the routes covering it", "SELECT prefix, plen, MAX(origin) AS origin, COUNT(*) AS n FROM bb_updates WHERE prefix IN (:c) GROUP BY prefix, plen ORDER BY plen DESC LIMIT 10", json!({"c": covering})).await;
        let roas = c.soft("the ROAs covering it", "SELECT prefix, asn, maxlen FROM bb_roa WHERE prefix IN (:c) ORDER BY plen DESC LIMIT 10", json!({"c": covering})).await;
        return Ok(json!({"kind": "prefix", "q": t, "prefix": p.to_string(),
            "routes": rows.iter().map(|r| json!({"prefix": r.s("prefix"), "origin": c.net(r.i("origin")), "updates": r.i("n")})).collect::<Vec<_>>(),
            "roas": roas.iter().map(|r| json!({"prefix": r.s("prefix"), "asn": r.i("asn"), "name": c.name(r.i("asn")), "maxlen": r.i("maxlen")})).collect::<Vec<_>>()}));
    }
    let digits = t.trim_start_matches(['A', 'S', 'a', 's']);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(a) = digits.parse::<i64>() {
            return Ok(json!({"kind": "asn", "q": t, "asn": a, "net": c.net(a)}));
        }
    }
    let words = t.replace(['\'', '"'], " ");
    let (nets, ixes) = tokio::join!(
        c.soft("networks by name (BM25)", "SELECT asn, short, country, kind, customers FROM bb_as WHERE MATCH(name, :q) ORDER BY bm25(name, :q) + 0.5 * ln(1 + customers) DESC LIMIT 12", json!({"q": words})),
        c.soft("exchanges by name (BM25)", "SELECT id, short, city, country, members FROM bb_ix WHERE MATCH(name, :q) ORDER BY bm25(name, :q) DESC LIMIT 6", json!({"q": words})),
    );
    Ok(json!({"kind": "words", "q": t,
        "nets": nets.iter().map(|r| json!({"asn": r.i("asn"), "name": r.s("short"), "country": r.s("country"), "kind": r.s("kind"), "customers": r.i("customers")})).collect::<Vec<_>>(),
        "ixes": ixes.iter().map(|r| json!({"id": r.i("id"), "name": r.s("short"), "city": r.s("city"), "country": r.s("country"), "members": r.i("members")})).collect::<Vec<_>>()}))
}

// ---------------------------------------------------------------- Diagnose

/// A country's stack, from the users down: what its probes measure (Users),
/// where its traceroutes jump (Path), whether its networks meet K's local
/// node (Peering), its routes (Routing), IODA (Outages), and K itself.
pub async fn diagnose(c: &Ctx<'_>, focus: &str) -> R<Value> {
    let (now_u, now_p) = c.clock().await;
    let since_atlas = iso(now_p - 20 * MIN);
    let since_tr = iso(now_p - 60 * MIN);
    let (pings, traces, alerts, outages, world) = tokio::join!(
        c.soft("each country's probes to K", "SELECT country, MEDIAN(rtt) AS med, COUNT(DISTINCT probe) AS probes, COUNT(DISTINCT probe) FILTER (WHERE rtt > 100) AS slow, AVG(loss) AS loss FROM bb_ping WHERE ts >= :since AND msm = 1001 GROUP BY country HAVING COUNT(DISTINCT probe) >= 5 LIMIT 300", json!({"since": since_atlas})),
        c.soft("each country's traceroutes that jump 80 ms or more", "SELECT country, COUNT(*) AS n, COUNT(*) FILTER (WHERE jump_ms >= 80) AS jumps FROM bb_trace WHERE ts >= :since AND msm = 5001 GROUP BY country LIMIT 300", json!({"since": since_tr})),
        c.soft("each country's open routing alerts", "SELECT country, COUNT(*) AS n, COUNT(*) FILTER (WHERE severity = 'bad') AS bad FROM bb_alerts WHERE state = 'open' AND rule NOT IN ('ioda', 'slow-country') GROUP BY country LIMIT 300", Value::Null),
        c.soft("each country's open outages: the whole country or a region, and single networks", "SELECT country, COUNT(*) AS n, COUNT(*) FILTER (WHERE kind IN ('country', 'region')) AS wide FROM bb_outage WHERE open GROUP BY country LIMIT 300", Value::Null),
        c.soft("the world's median", "SELECT MEDIAN(rtt) AS med FROM bb_ping WHERE ts >= :since AND msm = 1001 AND rtt IS NOT NULL", json!({"since": since_atlas})),
    );
    let tr: FxHashMap<String, (i64, i64)> = traces.iter().map(|r| (r.s("country"), (r.i("n"), r.i("jumps")))).collect();
    let al: FxHashMap<String, (i64, i64)> = alerts.iter().map(|r| (r.s("country"), (r.i("n"), r.i("bad")))).collect();
    let ou: FxHashMap<String, (i64, i64)> = outages.iter().map(|r| (r.s("country"), (r.i("n"), r.i("wide")))).collect();
    let mut list: Vec<&Row> = pings.iter().collect();
    // The slowest first, then the countries with the most probes.
    list.sort_by(|a, b| b.f("med").total_cmp(&a.f("med")));
    let mut chosen: Vec<&Row> = list.iter().take(8).copied().collect();
    let mut by_probes = pings.iter().collect::<Vec<_>>();
    by_probes.sort_by_key(|r| std::cmp::Reverse(r.i("probes")));
    for r in by_probes {
        if chosen.len() >= 14 {
            break;
        }
        if !chosen.iter().any(|x| x.s("country") == r.s("country")) {
            chosen.push(r);
        }
    }
    if !focus.is_empty() && !chosen.iter().any(|x| x.s("country") == focus) {
        if let Some(r) = pings.iter().find(|r| r.s("country") == focus) {
            chosen.insert(0, r);
        }
    }
    let cell = |state: &str, v: String, why: String| json!({"state": state, "v": v, "why": why});
    let cols: Vec<Value> = chosen
        .iter()
        .map(|r| {
            let cc = r.s("country");
            let (med, probes, slow) = (r.f("med"), r.i("probes"), r.i("slow"));
            let (tn, tj) = tr.get(&cc).copied().unwrap_or((0, 0));
            let (an, ab) = al.get(&cc).copied().unwrap_or((0, 0));
            let (on, wide) = ou.get(&cc).copied().unwrap_or((0, 0));
            let users = cell(if med > 150.0 { "bad" } else if med > 60.0 { "warn" } else { "ok" }, format!("{med:.0} ms"), format!("median of {probes} probes to K"));
            let path = if tn == 0 { cell("none", "–".into(), "no traceroutes".into()) } else { cell(if tj * 2 >= tn { "bad" } else if tj * 5 >= tn { "warn" } else { "ok" }, format!("{}%", tj * 100 / tn), format!("{tj} of {tn} traceroutes jump 80 ms or more at one hop")) };
            let peering = cell(if slow * 2 >= probes { "bad" } else if slow * 5 >= probes { "warn" } else { "ok" }, format!("{slow}"), format!("{slow} of {probes} probes over 100 ms: their networks do not reach a node near them"));
            let routing = cell(if ab > 0 { "bad" } else if an > 0 { "warn" } else { "ok" }, format!("{an}"), "open routing alerts of networks there".into());
            // A network of the country going dark is common; the whole
            // country or one of its regions is not.
            let outages = cell(if wide > 0 { "bad" } else if on > 0 { "warn" } else { "ok" }, format!("{on}"), if wide > 0 { format!("{on} IODA outages open, {wide} of the country or a region") } else { format!("{on} IODA outages open, each of one network") });
            let server = cell(if r.f("loss") > 0.5 { "bad" } else { "ok" }, format!("{:.0}%", r.f("loss") * 100.0), "pings K did not answer".into());
            // Lowest layer that is not ok: the start.
            let layers = [("server", &server), ("outages", &outages), ("routing", &routing), ("peering", &peering), ("path", &path), ("users", &users)];
            let start = layers.iter().find(|(_, c)| c["state"] == "bad").map(|(n, _)| *n);
            json!({"cc": cc, "probes": probes, "med": med, "start": start, "users": users, "path": path, "peering": peering, "routing": routing, "outages": outages, "server": server})
        })
        .collect();
    Ok(json!({"as_of": iso(now_u), "atlas_as_of": iso(now_p), "world": world.first().map(|r| r.f("med")), "columns": cols, "focus": focus}))
}

// ---------------------------------------------------------------- Rules, and how it is built

pub async fn rules(c: &Ctx<'_>) -> R<Value> {
    let (rules, counts) = tokio::join!(
        c.soft("the rules", "SELECT id, title, kind, severity, sql, about, last_run, last_ms, last_rows FROM bb_rules LIMIT 100", Value::Null),
        c.soft("open alerts per rule", "SELECT rule, COUNT(*) AS n FROM bb_alerts WHERE state = 'open' GROUP BY rule", Value::Null),
    );
    let open: FxHashMap<String, i64> = counts.iter().map(|r| (r.s("rule"), r.i("n"))).collect();
    let order: Vec<&str> = crate::rules::RULES.iter().map(|r| r.id).collect();
    let mut list: Vec<Value> = rules
        .iter()
        .map(|r| json!({"id": r.s("id"), "title": r.s("title"), "kind": r.s("kind"), "severity": r.s("severity"), "sql": r.s("sql"), "about": r.s("about"), "last_run": r.s("last_run"), "last_ms": r.f("last_ms"), "last_rows": r.i("last_rows"), "open": open.get(&r.s("id")).copied().unwrap_or(0)}))
        .collect();
    list.sort_by_key(|v| order.iter().position(|x| Some(*x) == v["id"].as_str()).unwrap_or(99));
    Ok(json!({"rules": list}))
}

async fn scale_counts(c: &Ctx<'_>) -> Value {
    let tables = ["bb_updates", "bb_as", "bb_p2c", "bb_p2p", "bb_ix", "bb_fac", "bb_member", "bb_roa", "bb_probe", "bb_ping", "bb_trace", "bb_outage", "bb_alerts", "bb_incidents"];
    let futs = tables.iter().map(|t| async move { (t.to_string(), c.soft(&format!("rows in {t}"), &format!("SELECT COUNT(*) AS n FROM {t}"), Value::Null).await.first().map(|r| r.i("n")).unwrap_or(0)) });
    let v = futures_util::future::join_all(futs).await;
    Value::Object(v.into_iter().map(|(k, n)| (k, json!(n))).collect())
}

pub async fn scale(c: &Ctx<'_>) -> R<Value> {
    let (now_u, _) = c.clock().await;
    let (counts, minutes, feed, rules, tables) = tokio::join!(
        scale_counts(c),
        c.soft("updates a minute, from the rollup bb_updates_1m", "SELECT bucket, SUM(count) AS n FROM bb_updates_1m WHERE bucket >= :since GROUP BY bucket ORDER BY bucket LIMIT 400", json!({"since": iso(now_u - 180 * MIN)})),
        c.soft("where each source stands", "SELECT id, kind, at, rows, note FROM bb_feed LIMIT 50", Value::Null),
        c.soft("each rule's last run", "SELECT id, title, last_ms, last_rows FROM bb_rules LIMIT 100", Value::Null),
        c.db.get("/v1/tables"),
    );
    let sizes: Vec<Value> = tables
        .ok()
        .and_then(|t| t["tables"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t["table"].as_str().is_some_and(|n| n.starts_with("bb_")))
        .map(|t| json!({"table": t["table"], "bytes": t["bytes"].as_i64().or(t["stats"]["bytes"].as_i64()), "docs": t["docs"].as_i64().or(t["stats"]["docs"].as_i64())}))
        .collect();
    Ok(json!({
        "as_of": iso(now_u), "counts": counts, "sizes": sizes,
        "minutes": minutes.iter().map(|r| json!({"t": r.s("bucket"), "n": r.i("n")})).collect::<Vec<_>>(),
        "feed": feed.iter().map(|r| json!({"id": r.s("id"), "kind": r.s("kind"), "at": r.s("at"), "rows": r.i("rows"), "note": r.s("note")})).collect::<Vec<_>>(),
        "rules": rules.iter().map(|r| json!({"id": r.s("id"), "title": r.s("title"), "ms": r.f("last_ms"), "rows": r.i("last_rows")})).collect::<Vec<_>>(),
    }))
}
