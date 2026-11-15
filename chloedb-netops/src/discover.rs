//! The network read back from what it reports (`--topology discovered`).
//!
//! Nobody describes this network to the app. It reads the feeds of
//! `feeds.rs` (BGP-LS, LLDP, SNMP and BMP, and the OLTs' ONT tables) and
//! builds from them alone what the inventory tables otherwise hold: sites,
//! devices, links and BGP sessions, and the estate behind the backbone.
//! Every site, device, link and session it writes says which feeds found it
//! and by what rule, in `netops_discovered`, and the Discovery page shows
//! them with the queries that read the feeds.
//!
//! - Devices: an SNMP agent whose sysLocation names a point of presence
//!   (its code, city, region and coordinates). What it is comes from
//!   sysDescr (the operating system, and whether it is a firewall), the
//!   vendor from sysObjectID's enterprise number, model, serial number and
//!   software from ENTITY-MIB; its number is its place in the management
//!   addresses' order.
//! - Links: LLDP neighbours (with both ports) and IS-IS adjacencies from
//!   BGP-LS (ports found by their addresses in IP-MIB), merged; capacity and
//!   delay from BGP-LS's TE attributes (else the port's speed, and a delay
//!   assumed inside a site), the plane from the administrative group, the
//!   carrier and circuit from the interfaces' descriptions.
//! - Roles, by where a device sits: a router with eBGP sessions in BMP is a
//!   border; one with an IS-IS adjacency to another site a core; any other
//!   IS-IS router an aggregation router; outside the IGP, a switch next to a
//!   firewall is a spine and the rest are leaves. A site with a fabric is a
//!   data centre, one with two borders a hub, the rest points of presence.
//! - The estate: hub routers are what LLDP shows hanging from the
//!   backbone, access nodes what hangs from hub routers; behind an OLT are
//!   the ONTs of its ONT table, behind any other access node its LLDP
//!   neighbours (a customer's name from the description of their port). The
//!   hub and the site each belongs to come from sysLocation, and a hub's
//!   power plant (no protocol says what it feeds) from its sysLocation too.
//!
//! The simulator's own numbers for how loaded a link is, how much traffic a
//! site draws and how fast a service answers are physics, not inventory:
//! they are given to the discovered network by id (`graft`) so that the
//! telemetry it plays is the same, and they never add a device or a link.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};

use crate::api::{Answer, Q, Shared};
use crate::estate::{Estate, Plan, PlanHub, PlanMetro, PlanObj, PlanSite};
use crate::feeds::{ENTERPRISES, GPON_VENDORS, LOCAL_ASN, PLANE_BITS};
use crate::sim::{Engine, Opts};
use crate::strata::{Error, Result, Row, RowExt, Strata, Trace};
use crate::telemetry::{iso, round};
use crate::world::{keep, World};

/// The feeds' rows, as read.
#[derive(Default)]
pub struct Read {
    pub system: Vec<Row>,
    pub ifaces: Vec<Row>,
    pub lldp: Vec<Row>,
    pub nodes: Vec<Row>,
    pub links: Vec<Row>,
    pub bmp: Vec<Row>,
    pub onts: Vec<Row>,
}

impl Read {
    /// Rows written as values (by `feeds.rs`), as a query returns them.
    pub fn of(f: crate::feeds::Feeds) -> Read {
        let rows = |v: Vec<Value>| v.into_iter().filter_map(|r| if let Value::Object(m) = r { Some(m) } else { None }).collect::<Vec<Row>>();
        Read { system: rows(f.snmp_system), ifaces: rows(f.snmp_ifaces), lldp: rows(f.lldp), nodes: rows(f.bgpls_nodes), links: rows(f.bgpls_links), bmp: rows(f.bmp_peers), onts: rows(f.gpon_onts) }
    }

    pub fn extend(&mut self, o: Read) {
        self.system.extend(o.system);
        self.ifaces.extend(o.ifaces);
        self.lldp.extend(o.lldp);
        self.nodes.extend(o.nodes);
        self.links.extend(o.links);
        self.bmp.extend(o.bmp);
        self.onts.extend(o.onts);
    }
}

/// An address as a number, for ordering (0 for what is not one).
fn ip_num(s: &str) -> u64 {
    let p: Vec<u64> = s.split('.').filter_map(|o| o.parse().ok()).collect();
    if p.len() == 4 {
        (p[0] << 24) | (p[1] << 16) | (p[2] << 8) | p[3]
    } else {
        0
    }
}

/// The last number in a text ("1/1/12" is 12, "Amsterdam hub 3" is 3).
fn last_num(s: &str) -> Option<usize> {
    s.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).last().and_then(|x| x.parse().ok())
}

/// The first number in a text ("Cabinet 5, Amsterdam hub 3" is 5).
fn first_num(s: &str) -> Option<usize> {
    s.split(|c: char| !c.is_ascii_digit()).find(|x| !x.is_empty()).and_then(|x| x.parse().ok())
}

/// A point of presence as sysLocation names it: `CODE | City, CC | Region | lat,lon`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pop {
    pub code: String,
    pub city: String,
    pub country: String,
    pub region: String,
    pub lat: f64,
    pub lon: f64,
}

pub fn pop_location(s: &str) -> Option<Pop> {
    let f: Vec<&str> = s.split('|').map(str::trim).collect();
    if f.len() != 4 {
        return None;
    }
    let (lat, lon) = f[3].split_once(',')?;
    let (city, country) = f[1].rsplit_once(',')?;
    Some(Pop { code: f[0].to_lowercase(), city: city.trim().into(), country: country.trim().into(), region: f[2].into(), lat: lat.trim().parse().ok()?, lon: lon.trim().parse().ok()? })
}

/// Where an object of the access network is: `CODE | hub | site` (a hub's
/// own routers and power plant have no site).
fn access_location(s: &str) -> Option<(String, String, Option<String>)> {
    let f: Vec<&str> = s.split('|').map(str::trim).collect();
    match f.len() {
        2 => Some((f[0].to_lowercase(), f[1].into(), None)),
        3 => Some((f[0].to_lowercase(), f[1].into(), Some(f[2].into()))),
        _ => None,
    }
}

/// The vendor a sysObjectID names, by its enterprise number.
pub fn vendor_of(oid: &str) -> &'static str {
    let pen = oid.strip_prefix("1.3.6.1.4.1.").and_then(|r| r.split('.').next()).and_then(|p| p.parse::<u32>().ok());
    pen.and_then(|p| ENTERPRISES.iter().find(|e| e.0 == p)).map_or("", |e| e.1)
}

/// The operating system a sysDescr names.
pub fn os_of(descr: &str) -> &'static str {
    if descr.contains("JUNOS") {
        "Junos"
    } else if descr.contains("IOS XR") {
        "IOS XR"
    } else if descr.contains("IOS XE") {
        "IOS XE"
    } else if descr.contains("Arista Networks EOS") {
        "EOS"
    } else if descr.contains("Palo Alto Networks") {
        "PAN-OS"
    } else if descr.contains("TiMOS") {
        "SR OS"
    } else if descr.contains("RouterOS") {
        "RouterOS"
    } else {
        ""
    }
}

fn plane_of(group: i64) -> &'static str {
    PLANE_BITS.iter().find(|p| group & p.0 != 0).map_or("", |p| p.1)
}

/// A role in words: "an aggregation router", or without the article.
fn role_name(role: &str, article: bool) -> String {
    let (a, name) = match role {
        "agg" => ("an", "aggregation router"),
        "core" => ("a", "core router"),
        "border" => ("a", "border router"),
        "firewall" => ("a", "firewall"),
        "spine" => ("a", "spine switch"),
        "leaf" => ("a", "leaf switch"),
        x => ("a", x),
    };
    if article { format!("{a} {name}") } else { name.to_string() }
}

/// "1 cabinet", "7 cabinets".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn tags(role: &str) -> Vec<&'static str> {
    match role {
        "core" => vec!["backbone", "mpls"],
        "border" => vec!["edge", "bgp"],
        "agg" => vec!["access", "customers"],
        "firewall" => vec!["security"],
        _ => vec!["fabric"],
    }
}

/// What discovery found on the backbone: the inventory's rows, and how
/// each was found.
#[derive(Default)]
pub struct Found {
    pub sites: Vec<Value>,
    pub devices: Vec<Value>,
    pub links: Vec<Value>,
    pub peers: Vec<Value>,
    pub how: Vec<Value>,
}

struct Dev {
    agent: String,
    pop: Pop,
    descr: String,
    oid: String,
    model: String,
    serial: String,
    sw: String,
}

#[derive(Default)]
struct Seen {
    a: String,
    b: String,
    a_if: String,
    b_if: String,
    lldp_from: BTreeSet<String>,
    isis: usize,
    bw: i64,
    delay: Vec<i64>,
    group: i64,
    metric: i64,
}

fn how(kind: &str, item: &str, sources: &[&str], text: String, at: &str) -> Value {
    json!({"id": format!("{kind}:{item}"), "kind": kind, "item": item, "sources": sources, "how": text, "at": at})
}

/// The backbone from the feeds: sites, devices, links and BGP sessions.
pub fn backbone(r: &Read, now: i64) -> Found {
    let at = iso(now);
    // The boxes at points of presence.
    let mut devs: BTreeMap<String, Dev> = BTreeMap::new();
    for row in &r.system {
        let Some(pop) = pop_location(&row.s("sys_location")) else { continue };
        devs.insert(row.s("sys_name"), Dev { agent: row.s("id"), pop, descr: row.s("sys_descr"), oid: row.s("sys_object_id"), model: row.s("model"), serial: row.s("serial"), sw: row.s("sw_rev") });
    }
    let by_agent: HashMap<String, String> = devs.iter().map(|(n, d)| (d.agent.clone(), n.clone())).collect();
    // Numbered in the order of their management addresses.
    let mut order: Vec<&String> = devs.keys().collect();
    order.sort_by_key(|n| (ip_num(&devs[*n].agent), (*n).clone()));
    let num: HashMap<String, i64> = order.iter().enumerate().map(|(i, n)| ((*n).clone(), i as i64 + 1)).collect();
    // Interfaces: their names, speeds, descriptions and addresses.
    let (mut port_of_ip, mut loopback, mut speed, mut alias) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
    for row in &r.ifaces {
        let Some(dev) = by_agent.get(&row.s("agent")) else { continue };
        let (name, ip) = (row.s("if_name"), row.s("ip"));
        if row.i("prefix_len") == 32 && !ip.is_empty() {
            loopback.insert(dev.clone(), ip.clone());
        } else if !ip.is_empty() {
            port_of_ip.insert(ip, (dev.clone(), name.clone()));
        }
        speed.insert((dev.clone(), name.clone()), row.i("high_speed"));
        let a = row.s("if_alias");
        if !a.is_empty() {
            alias.insert((dev.clone(), name), a);
        }
    }
    // The IS-IS database: its routers by router id.
    let mut name_of_rid: HashMap<String, String> = HashMap::new();
    for row in &r.nodes {
        let host = row.s("hostname");
        if devs.contains_key(&host) {
            name_of_rid.insert(row.s("id"), host.clone());
            loopback.entry(host).or_insert(row.s("id"));
        }
    }
    let igp: HashSet<String> = name_of_rid.values().cloned().collect();
    for (dev, ip) in &loopback {
        name_of_rid.entry(ip.clone()).or_insert(dev.clone());
    }
    // Links: LLDP's neighbours and IS-IS's adjacencies, merged.
    let rank = |n: &str| (num.get(n).copied().unwrap_or(i64::MAX), n.to_string());
    let mut seen: BTreeMap<(String, String, String, String), Seen> = BTreeMap::new();
    let key = |x: &str, xp: &str, y: &str, yp: &str| if rank(x) <= rank(y) { (x.to_string(), xp.to_string(), y.to_string(), yp.to_string()) } else { (y.to_string(), yp.to_string(), x.to_string(), xp.to_string()) };
    for row in &r.lldp {
        let (Some(me), other) = (by_agent.get(&row.s("agent")), row.s("rem_sys_name")) else { continue };
        if !devs.contains_key(&other) || *me == other {
            continue;
        }
        let k = key(me, &row.s("local_port"), &other, &row.s("rem_port"));
        let s = seen.entry(k.clone()).or_default();
        (s.a, s.a_if, s.b, s.b_if) = (k.0, k.1, k.2, k.3);
        s.lldp_from.insert(me.clone());
    }
    for row in &r.links {
        let (Some(me), Some(other)) = (name_of_rid.get(&row.s("local_node")), name_of_rid.get(&row.s("remote_node"))) else { continue };
        let my_port = port_of_ip.get(&row.s("local_ip")).filter(|p| p.0 == *me).map(|p| p.1.clone());
        let their_port = port_of_ip.get(&row.s("remote_ip")).filter(|p| p.0 == *other).map(|p| p.1.clone());
        let k = match (my_port, their_port) {
            (Some(a), Some(b)) => key(me, &a, other, &b),
            // Without the addresses' ports, the one link LLDP saw between the two.
            _ => {
                let pair: Vec<_> = seen.keys().filter(|k| (k.0 == *me && k.2 == *other) || (k.0 == *other && k.2 == *me)).cloned().collect();
                if pair.len() == 1 { pair[0].clone() } else { key(me, "", other, "") }
            }
        };
        let s = seen.entry(k.clone()).or_default();
        (s.a, s.a_if, s.b, s.b_if) = (k.0, k.1, k.2, k.3);
        s.isis += 1;
        s.bw = s.bw.max(row.i("max_bw_mbps"));
        if row.get("delay_us").is_some_and(|v| !v.is_null()) {
            s.delay.push(row.i("delay_us"));
        }
        s.group |= row.i("admin_group");
        s.metric = s.metric.max(row.i("igp_metric"));
    }
    // BGP sessions, by the router that reports them.
    let mut sessions: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for row in &r.bmp {
        if let Some(dev) = name_of_rid.get(&row.s("router")) {
            sessions.entry(dev.clone()).or_default().push(row);
        }
    }
    let ebgp = |n: &str| sessions.get(n).map_or(0, |s| s.iter().filter(|p| p.i("peer_asn") != if p.i("local_asn") > 0 { p.i("local_asn") } else { LOCAL_ASN }).count());
    // Roles, by where each sits.
    let mut nbrs: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for s in seen.values() {
        nbrs.entry(&s.a).or_default().insert(&s.b);
        nbrs.entry(&s.b).or_default().insert(&s.a);
    }
    let firewall = |n: &str| devs.get(n).is_some_and(|d| d.descr.to_lowercase().contains("firewall"));
    let mut role: BTreeMap<String, (&'static str, String)> = BTreeMap::new();
    for (n, d) in &devs {
        let near = nbrs.get(n.as_str()).cloned().unwrap_or_default();
        let away: BTreeSet<String> = seen.values().filter(|s| s.isis > 0 && (s.a == *n || s.b == *n)).map(|s| if s.a == *n { &s.b } else { &s.a }).filter_map(|o| devs.get(o)).filter(|o| o.pop.code != d.pop.code).map(|o| o.pop.code.to_uppercase()).collect();
        let r = if firewall(n) {
            ("firewall", "its sysDescr says it is a firewall".to_string())
        } else if igp.contains(n) && ebgp(n) > 0 {
            ("border", format!("in the IS-IS database, with {} eBGP sessions in BMP", ebgp(n)))
        } else if igp.contains(n) && !away.is_empty() {
            ("core", format!("in the IS-IS database, adjacent to routers at {}", away.into_iter().collect::<Vec<_>>().join(", ")))
        } else if igp.contains(n) {
            ("agg", format!("in the IS-IS database, adjacent only inside {} and with no eBGP session", d.pop.code.to_uppercase()))
        } else if let Some(f) = near.iter().find(|m| firewall(m) || igp.contains(**m)) {
            ("spine", format!("not in the IGP; an LLDP neighbour of {f}"))
        } else {
            ("leaf", format!("not in the IGP; its LLDP neighbours are {}", near.iter().copied().collect::<Vec<_>>().join(", ")))
        };
        role.insert(n.clone(), r);
    }
    let mut f = Found::default();
    // Sites, in the order of their first management address.
    let mut sites: Vec<(&str, i64)> = Vec::new();
    for n in &order {
        let code = devs[*n].pop.code.as_str();
        if !sites.iter().any(|s| s.0 == code) {
            sites.push((code, num[*n]));
        }
    }
    for (code, _) in &sites {
        let here: Vec<&String> = order.iter().copied().filter(|n| devs[*n].pop.code == *code).collect();
        let p = &devs[here[0]].pop;
        let count = |r: &str| here.iter().filter(|n| role[**n].0 == r).count();
        let (kind, why) = if count("spine") + count("leaf") > 0 {
            ("dc", format!("a data centre: it has a fabric ({} spines, {} leaves)", count("spine"), count("leaf")))
        } else if count("border") >= 2 {
            ("hub", format!("a hub: {} border routers", count("border")))
        } else {
            ("pop", "a point of presence: no fabric, one border router".to_string())
        };
        f.sites.push(json!({"id": code, "name": p.city, "city": p.city, "country": p.country, "region": p.region, "kind": kind, "lat": p.lat, "lon": p.lon, "devices": here.len()}));
        f.how.push(how("site", code, &["SNMP"], format!("{} devices name it in their sysLocation ({}, {}, {}, at {:.2}, {:.2}); {why}", here.len(), p.city, p.country, p.region, p.lat, p.lon), &at));
    }
    for n in &order {
        let d = &devs[*n];
        let (r, why) = &role[*n];
        let (vendor, os) = (vendor_of(&d.oid), os_of(&d.descr));
        f.devices.push(json!({"id": n, "num": num[*n], "site": d.pop.code, "role": r, "vendor": vendor, "model": d.model, "os": os, "os_version": d.sw, "mgmt_ip": d.agent,
            "loopback": loopback.get(*n).cloned().unwrap_or_default(), "asn": LOCAL_ASN, "status": "up", "serial": d.serial, "tags": tags(r)}));
        let mut src = vec!["SNMP", "ENTITY-MIB"];
        if igp.contains(*n) {
            src.push("BGP-LS");
        }
        if nbrs.contains_key(n.as_str()) {
            src.push("LLDP");
        }
        if ebgp(n) > 0 {
            src.push("BMP");
        }
        f.how.push(how("device", n, &src, format!("sysName {n} at {}; {vendor} by its sysObjectID, {os} {} by sysDescr and ENTITY-MIB, model {} and serial {} by ENTITY-MIB; at {} by its sysLocation; {}: {why}",
            d.agent, d.sw, d.model, d.serial, d.pop.code.to_uppercase(), role_name(r, false)), &at));
    }
    let mut ids: HashSet<String> = HashSet::new();
    for (k, s) in seen.values().enumerate() {
        let (Some(a), Some(b)) = (devs.get(&s.a), devs.get(&s.b)) else { continue };
        let (ra, rb) = (role[&s.a].0, role[&s.b].0);
        let kind = if a.pop.code != b.pop.code {
            "backbone"
        } else if matches!((ra, rb), ("spine", "leaf") | ("leaf", "spine")) {
            "fabric"
        } else if matches!((ra, rb), ("agg", "core") | ("core", "agg") | ("firewall", "core") | ("core", "firewall") | ("firewall", "spine") | ("spine", "firewall")) {
            "uplink"
        } else {
            "core"
        };
        let port_speed = speed.get(&(s.a.clone(), s.a_if.clone())).or_else(|| speed.get(&(s.b.clone(), s.b_if.clone()))).copied().unwrap_or(0);
        let capacity = if s.bw > 0 { s.bw } else { port_speed };
        let (latency, delay_why) = if s.delay.is_empty() {
            (0.1, "no delay measured: 0.1 ms assumed inside a site".to_string())
        } else {
            let ms = round(s.delay.iter().sum::<i64>() as f64 / s.delay.len() as f64 / 1000.0, 2);
            (ms, format!("{ms} ms by BGP-LS's delay"))
        };
        let plane = plane_of(s.group);
        let note = alias.get(&(s.a.clone(), s.a_if.clone())).or_else(|| alias.get(&(s.b.clone(), s.b_if.clone()))).cloned().unwrap_or_default();
        let parts: Vec<&str> = note.split('|').map(str::trim).collect();
        let (provider, circuit) = if parts.len() >= 2 { (parts[0].to_string(), parts[1].to_string()) } else { (String::new(), String::new()) };
        let mut id = format!("{}--{}", s.a, s.b);
        if !ids.insert(id.clone()) {
            id = format!("{id}#{k}");
            ids.insert(id.clone());
        }
        f.links.push(json!({"id": id, "src": s.a, "dst": s.b, "num": k as i64 + 1, "kind": kind, "a_if": s.a_if, "z_if": s.b_if, "capacity_mbps": capacity, "circuit": circuit, "provider": provider,
            "plane": plane, "status": "up", "latency_ms": latency, "a_site": a.pop.code, "z_site": b.pop.code}));
        let mut src = Vec::new();
        let mut text = Vec::new();
        if !s.lldp_from.is_empty() {
            src.push("LLDP");
            text.push(if s.lldp_from.len() >= 2 { format!("LLDP from both ends ({} to {})", s.a_if, s.b_if) } else { format!("LLDP from {} only", s.lldp_from.iter().next().cloned().unwrap_or_default()) });
        }
        if s.isis > 0 {
            src.push("BGP-LS");
            text.push(format!("an IS-IS adjacency {} ({} Gb/s, metric {}, {delay_why})", if s.isis >= 2 { "both ways" } else { "one way" }, capacity / 1000, s.metric));
            if !plane.is_empty() {
                text.push(format!("plane {plane} by its administrative group (0x{:x})", s.group));
            }
        } else {
            text.push(format!("{} Gb/s by the port's speed (IF-MIB); {delay_why}", capacity / 1000));
        }
        if !circuit.is_empty() {
            src.push("IF-MIB");
            text.push(format!("circuit {circuit} of {provider} from the interfaces' descriptions"));
        }
        text.push(format!("{kind}: {}", match kind {
            "backbone" => format!("its ends are at {} and {}", a.pop.code.to_uppercase(), b.pop.code.to_uppercase()),
            "fabric" => "a spine to a leaf".into(),
            "uplink" => format!("{} to {}", role_name(ra, true), role_name(rb, true)),
            _ => format!("between two routers at {}", a.pop.code.to_uppercase()),
        }));
        f.how.push(how("link", &id, &src, text.join("; "), &at));
    }
    for (dev, list) in &sessions {
        for p in list {
            let asn = p.i("peer_asn");
            let rib = p.i("adj_rib_in");
            let kind = if rib >= 500_000 { "transit" } else { "peer" };
            let name = if p.s("peer_desc").is_empty() { format!("AS{asn}") } else { p.s("peer_desc") };
            let id = format!("{dev}:as{asn}");
            f.peers.push(json!({"id": id, "device": dev, "peer_ip": p.s("peer_ip"), "peer_asn": asn, "peer_name": name, "kind": kind, "state": if p.s("state") == "up" { "established" } else { "idle" },
                "prefixes_rx": rib, "prefixes_tx": Value::Null, "since": p.get("up_since").cloned().unwrap_or(Value::Null)}));
            f.how.push(how("peer", &id, &["BMP"], format!("{dev} reports a session with {} (AS{asn}) at {}; {kind}: {} routes taken from it{}", name, p.s("peer_ip"), crate::estate::thousands(rib.max(0) as usize),
                if kind == "transit" { ", a full table" } else { "" }), &at));
        }
    }
    f
}

/// The estate behind the backbone of `w`, from the feeds: what to lay out,
/// and how each hub was found. `base` is the simulator's round trip of
/// each object (physics), by id.
pub fn estate_plan(r: &Read, w: &World, base: &HashMap<String, f64>) -> (Plan, Vec<Value>) {
    struct Obj<'a> {
        row: &'a Row,
        code: String,
        hub: String,
        site: Option<String>,
    }
    let backbone: HashSet<&str> = w.devices.iter().map(|d| d.id.as_str()).collect();
    let mut objs: HashMap<String, Obj> = HashMap::new();
    let mut by_agent: HashMap<String, String> = HashMap::new();
    for row in &r.system {
        let name = row.s("sys_name");
        if backbone.contains(name.as_str()) {
            continue;
        }
        let Some((code, hub, site)) = access_location(&row.s("sys_location")) else { continue };
        by_agent.insert(row.s("id"), name.clone());
        objs.insert(name, Obj { row, code, hub, site });
    }
    // LLDP from the access network's agents: (port number, port, its description, the neighbour).
    let mut lldp: HashMap<String, Vec<(usize, String, String)>> = HashMap::new();
    for row in &r.lldp {
        let Some(me) = by_agent.get(&row.s("agent")) else { continue };
        let port = row.s("local_port");
        lldp.entry(me.clone()).or_default().push((last_num(&port).unwrap_or(0) + 1000 * first_num(&port).unwrap_or(0), row.s("local_port_desc"), row.s("rem_sys_name")));
    }
    for l in lldp.values_mut() {
        l.sort();
    }
    let hub_routers: BTreeSet<String> = lldp.iter().filter(|(_, l)| l.iter().any(|x| backbone.contains(x.2.as_str()))).map(|(n, _)| n.clone()).collect();
    let access: BTreeSet<String> = lldp.iter().filter(|(n, l)| !hub_routers.contains(*n) && l.iter().any(|x| hub_routers.contains(&x.2))).map(|(n, _)| n.clone()).collect();
    let power: BTreeSet<String> = objs.iter().filter(|(_, o)| { let d = o.row.s("sys_descr").to_lowercase(); d.contains("rectifier") || d.contains("power system") }).map(|(n, _)| n.clone()).collect();
    // The ONTs of each OLT, by its agent.
    let mut onts: HashMap<String, Vec<&Row>> = HashMap::new();
    for row in &r.onts {
        if let Some(olt) = by_agent.get(&row.s("agent")) {
            onts.entry(olt.clone()).or_default().push(row);
        }
    }
    for l in onts.values_mut() {
        l.sort_by_key(|o| (last_num(&o.s("pon_port")).unwrap_or(0), o.i("ont_id")));
    }
    let obj = |name: &str, display: String, vendor: &'static str, model: String, version: String, customer: String| PlanObj {
        id: name.to_string(), name: display, vendor, model: keep(&model), version, customer, base_ms: base.get(name).copied().unwrap_or(1.0),
    };
    let snmp_obj = |name: &str, display: String| {
        let o = &objs[name];
        obj(name, display, vendor_of(&o.row.s("sys_object_id")), o.row.s("model"), o.row.s("sw_rev"), String::new())
    };
    // Hubs: by the metro their routers hang from and the hub sysLocation names.
    let mut hubs: BTreeMap<(String, String), (Vec<String>, Option<String>, Vec<String>)> = BTreeMap::new();
    for n in &hub_routers {
        let o = &objs[n];
        let metro = lldp[n].iter().find_map(|x| w.device(&x.2)).map_or(o.code.clone(), |d| d.site.to_string());
        hubs.entry((metro, o.hub.clone())).or_default().0.push(n.clone());
    }
    // A hub's metro is where its routers hang from; what has no router of its own goes by its sysLocation.
    let metro_of: HashMap<String, String> = hubs.keys().map(|(m, h)| (h.clone(), m.clone())).collect();
    let metro_by_hub = |code: &str, hub: &str| metro_of.get(hub).cloned().unwrap_or(code.to_string());
    for n in &power {
        let o = &objs[n];
        let m = metro_by_hub(&o.code, &o.hub);
        hubs.entry((m, o.hub.clone())).or_default().1 = Some(n.clone());
    }
    for n in &access {
        let o = &objs[n];
        let m = metro_by_hub(&o.code, &o.hub);
        hubs.entry((m, o.hub.clone())).or_default().2.push(n.clone());
    }
    let mut metros: BTreeMap<String, Vec<(usize, PlanHub)>> = BTreeMap::new();
    let mut how_rows = Vec::new();
    for ((metro, hub_name), (mut routers, pw, nodes)) in hubs {
        routers.sort();
        let number = last_num(&hub_name).unwrap_or(metros.get(&metro).map_or(0, |v| v.len()) + 1);
        let tag = format!("h{number:02}-{metro}");
        let uplinks: Vec<Vec<String>> = routers.iter().map(|r| lldp[r].iter().filter(|x| backbone.contains(x.2.as_str())).map(|x| x.2.clone()).collect()).collect();
        let router_objs: Vec<PlanObj> = routers.iter().map(|r| snmp_obj(r, r.clone())).collect();
        let power = pw.as_ref().map(|p| snmp_obj(p, format!("Power plant, {hub_name}")));
        // Its sites, by the site sysLocation names.
        let mut sites: Vec<(usize, PlanSite)> = Vec::new();
        let (mut n_ont, mut n_cpe, mut n_radio) = (0, 0, 0);
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for n in &nodes {
            let o = &objs[n];
            let site_name = o.site.clone().unwrap_or_default();
            let s_num = first_num(&site_name).unwrap_or(sites.len() + 1);
            let descr = o.row.s("sys_descr");
            let kind: &'static str = if descr.contains("OLT") { "olt" } else if descr.contains("cell site") { "cell" } else { "switch" };
            *kinds.entry(kind).or_default() += 1;
            let homes: Vec<usize> = lldp[n].iter().filter_map(|x| routers.iter().position(|r| *r == x.2)).collect();
            let (behind_kind, behind): (&'static str, Vec<PlanObj>) = match kind {
                "olt" => ("ont", onts.get(n).into_iter().flatten().enumerate().map(|(k, row)| {
                    let serial = row.s("serial");
                    let vendor = GPON_VENDORS.iter().find(|g| serial.starts_with(g.0)).map_or("", |g| g.1);
                    obj(&row.s("description"), format!("ONT {}", k + 1), vendor, row.s("model"), row.s("version"), String::new())
                }).collect()),
                _ => {
                    let downs: Vec<&(usize, String, String)> = lldp[n].iter().filter(|x| !routers.contains(&x.2) && objs.contains_key(&x.2)).collect();
                    let bk = if kind == "cell" { "radio" } else { "cpe" };
                    (bk, downs.iter().enumerate().map(|(k, x)| {
                        let customer = x.1.strip_prefix("CUST ").unwrap_or("").to_string();
                        let display = if bk == "radio" { format!("Sector {}", k + 1) } else { format!("{customer}, customer router") };
                        let mut p = snmp_obj(&x.2, display);
                        p.customer = customer;
                        p
                    }).collect())
                }
            };
            match behind_kind {
                "ont" => n_ont += behind.len(),
                "radio" => n_radio += behind.len(),
                _ => n_cpe += behind.len(),
            }
            let node = snmp_obj(n, n.clone());
            sites.push((s_num, PlanSite { tag: format!("s{s_num:02}-{tag}"), name: site_name, kind, node, homes, behind_kind, behind }));
        }
        sites.sort_by_key(|s| s.0);
        let k = |x: &str| *kinds.get(x).unwrap_or(&0);
        let text = format!("{} ({}) by LLDP toward {}; {}; {} by their sysLocation: {} with {} from their OLTs' ONT tables, {} with {} and {} with {} by LLDP",
            count(routers.len(), "hub router", "hub routers"), routers.join(", "), uplinks.iter().flatten().collect::<BTreeSet<_>>().into_iter().cloned().collect::<Vec<_>>().join(", "),
            pw.as_ref().map_or("no power plant reports from it".to_string(), |p| format!("power plant {p} by its sysLocation")),
            count(sites.len(), "site", "sites"), count(k("olt"), "cabinet", "cabinets"), count(n_ont, "ONT", "ONTs"), count(k("switch"), "business park", "business parks"), count(n_cpe, "customer router", "customer routers"),
            count(k("cell"), "cell site", "cell sites"), count(n_radio, "radio", "radios"));
        how_rows.push(json!({"id": format!("hub:{tag}"), "kind": "hub", "item": tag, "sources": ["LLDP", "SNMP", "GPON"], "how": text}));
        metros.entry(metro.clone()).or_default().push((number, PlanHub { tag, name: hub_name, routers: router_objs, uplinks, power, sites: sites.into_iter().map(|s| s.1).collect() }));
    }
    let mut plan = Plan::default();
    for (metro, mut list) in metros {
        list.sort_by_key(|h| h.0);
        let core_ms = w.devices.iter().filter(|d| d.site == metro).map(|d| (d.id.clone(), base.get(&d.id).copied().unwrap_or(0.5))).collect();
        plan.metros.push(PlanMetro { site: metro, core_ms, hubs: list.into_iter().map(|h| h.1).collect() });
    }
    (plan, how_rows)
}

/// Gives the discovered network the simulator's physics, by id: how
/// loaded each link is at the daily peak, how much traffic each site draws
/// and in which time zone, how many requests each service takes and how
/// fast it answers. Nothing is added: a link, site or service the feeds
/// did not show stays out.
pub fn graft(w: &mut World, truth: &World) {
    for s in &mut w.sites {
        if let Some(t) = truth.sites.iter().find(|t| t.id == s.id) {
            s.tz = t.tz;
            s.weight = t.weight;
        }
    }
    let pair = |a: &str, b: &str| if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) };
    let load: HashMap<(String, String), f64> = truth.links.iter().map(|l| (pair(&truth.devices[l.a].id, &truth.devices[l.b].id), l.load)).collect();
    let keys: Vec<(String, String)> = w.links.iter().map(|l| pair(&w.devices[l.a].id, &w.devices[l.b].id)).collect();
    for (l, k) in w.links.iter_mut().zip(keys) {
        if let Some(v) = load.get(&k) {
            l.load = *v;
        }
    }
    for s in &mut w.services {
        if let Some(t) = truth.services.iter().find(|t| t.id == s.id) {
            s.rps = t.rps;
            s.base_latency_ms = t.base_latency_ms;
        }
    }
}

// ---- reading and writing

/// Rows a page of a scroll of the feeds reads.
const PAGE: usize = 10_000;

/// Every row of a feed, read as a scroll: a page at a time, all from one
/// version of the table (a query alone answers at most 10,000 rows).
async fn scan(db: &Strata, trace: &Trace, table: &str, cols: &str) -> Result<Vec<Row>> {
    let sql = format!("SELECT {cols} FROM {table}");
    let t0 = std::time::Instant::now();
    let rows = |j: &mut Value| -> Vec<Row> {
        match j.get_mut("rows").map(Value::take) {
            Some(Value::Array(a)) => a.into_iter().filter_map(|r| if let Value::Object(m) = r { Some(m) } else { None }).collect(),
            _ => Vec::new(),
        }
    };
    let mut j = db.post("/v1/query", &json!({"sql": sql, "objects": true, "page_size": PAGE})).await?;
    let mut out = rows(&mut j);
    let mut pages = 1;
    while let Some(cursor) = j["cursor"].as_str().map(str::to_string) {
        j = db.post("/v1/query", &json!({"cursor": cursor, "objects": true})).await?;
        out.extend(rows(&mut j));
        pages += 1;
    }
    let what = format!("Read {table}, as a scroll ({pages} page{} of at most {} rows)", if pages == 1 { "" } else { "s" }, crate::estate::thousands(PAGE));
    trace.push(&what, &sql, &json!({"page_size": PAGE}), out.len(), t0.elapsed().as_secs_f64() * 1000.0, None);
    Ok(out)
}

/// Every feed, as read.
pub async fn read(db: &Strata, trace: &Trace) -> Result<Read> {
    Ok(Read {
        system: scan(db, trace, "netops_snmp_system", "id, sys_name, sys_descr, sys_object_id, sys_location, model, serial, sw_rev").await?,
        ifaces: scan(db, trace, "netops_snmp_ifaces", "id, agent, if_name, if_alias, high_speed, ip, prefix_len").await?,
        lldp: scan(db, trace, "netops_lldp", "id, agent, local_port, local_port_desc, rem_sys_name, rem_port").await?,
        nodes: scan(db, trace, "netops_bgpls_nodes", "id, hostname").await?,
        links: scan(db, trace, "netops_bgpls_links", "id, local_node, remote_node, local_ip, remote_ip, igp_metric, max_bw_mbps, delay_us, admin_group").await?,
        bmp: scan(db, trace, "netops_bmp_peers", "id, router, local_asn, peer_ip, peer_asn, peer_desc, state, adj_rib_in, up_since").await?,
        onts: scan(db, trace, "netops_gpon_onts", "id, agent, pon_port, ont_id, serial, model, version, description").await?,
    })
}

async fn state(db: &Strata, id: &str) -> String {
    db.query("SELECT val FROM netops_state WHERE id = :id", json!({"id": id})).await.ok().and_then(|r| r.first().map(|r| r.s("val"))).unwrap_or_default()
}

/// Refuses a database made the other way: the made-up inventory and the
/// discovered network use the same tables, and do not mix.
pub async fn check_mode(db: &Strata, discovered: bool) -> Result<()> {
    let mode = state(db, "topology").await;
    let made = !state(db, "sim").await.is_empty();
    let was = if mode == "discovered" { "discovered" } else { "inventory" };
    if made && (was == "discovered") != discovered {
        return Err(Error { status: 0, message: format!("this database was made with --topology {was}: start with --topology {was} again, with another --local directory, or add --reset to make it again") });
    }
    Ok(())
}

/// Sets the tables up for `--topology discovered` and reads the network
/// from the feeds. With the simulator on, its network first reports itself
/// into the feeds (once), with the business records no protocol carries:
/// services and customers. Returns the network the app works from and, with
/// the simulator on, the estate behind it.
pub async fn prepare(db: &Strata, opts: &Opts, say: impl Fn(&str)) -> Result<(World, Option<Estate>)> {
    db.health().await?;
    if opts.reset {
        say("dropping the tables");
        for name in crate::schema::names().iter().rev() {
            let _ = db.sql(&format!("DROP TABLE IF EXISTS {name}")).await;
        }
    }
    for (_, ddl, _) in crate::schema::tables() {
        if let Err(e) = db.sql(&ddl).await {
            let Some(at) = ddl.find("emb VECTOR") else { return Err(e) };
            db.sql(&format!("{}\n)", ddl[..at].trim_end().trim_end_matches(','))).await?;
        }
    }
    check_mode(db, true).await?;
    let now = crate::sim::minute_now();
    let truth = opts.simulate.then(World::build);
    let truth_est = truth.as_ref().filter(|_| opts.estate > 0).map(|t| Estate::build(t, opts.estate));
    if let Some(t) = &truth {
        let want = json!({"devices": t.devices.len(), "links": t.links.len(), "estate": truth_est.as_ref().map_or(0, |e| e.nodes.len())}).to_string();
        if state(db, "feeds").await != want {
            say("the simulated network reports itself: SNMP, LLDP, BGP-LS, BMP and ONT tables");
            for name in crate::schema::FEEDS {
                let ddl = crate::schema::tables().into_iter().find(|x| x.0 == *name).map(|x| x.1).unwrap_or_default();
                db.sql(&format!("DROP TABLE IF EXISTS {name}")).await?;
                db.sql(&ddl).await?;
            }
            for (table, rows) in crate::feeds::backbone(t, now).tables() {
                db.upsert(table, rows).await?;
            }
            if let Some(est) = &truth_est {
                const PART: usize = 20_000;
                let mut from = 0;
                while from < est.nodes.len() {
                    let to = (from + PART).min(est.nodes.len());
                    say(&format!("the estate reports itself: objects {} to {} of {}", crate::estate::thousands(from), crate::estate::thousands(to), crate::estate::thousands(est.nodes.len())));
                    for (table, rows) in crate::feeds::estate(t, est, from..to, now).tables() {
                        if !rows.is_empty() {
                            db.upsert(table, rows).await?;
                        }
                    }
                    from = to;
                }
            }
            // What no protocol carries: what is sold, and to whom.
            db.upsert("netops_services", t.service_rows()).await?;
            db.upsert("netops_customers", t.customer_rows()).await?;
            db.upsert("netops_state", vec![json!({"id": "feeds", "val": want, "updated": iso(now)})]).await?;
        }
    }
    say("reading the network back from the feeds");
    let trace = Trace::new();
    let t0 = std::time::Instant::now();
    let r = read(db, &trace).await?;
    let found = backbone(&r, now);
    if found.devices.is_empty() {
        return Err(Error { status: 0, message: "the feeds name no device at a point of presence: write netops_snmp_system (with sysLocation \"CODE | City, CC | Region | lat,lon\"), netops_lldp, netops_bgpls_nodes and netops_bgpls_links, and start again".into() });
    }
    let base: HashMap<String, f64> = truth_est.as_ref().map(|e| e.nodes.iter().map(|n| (n.id.clone(), n.base_ms)).collect()).unwrap_or_default();
    // What was found: the same again on a restart leaves the inventory (and
    // what is down in it) as it was.
    let signature = json!({"sites": found.sites.len(), "devices": found.devices.len(), "links": found.links.len(), "peers": found.peers.len(), "feeds": r.system.len() + r.lldp.len() + r.onts.len() + r.links.len()});
    let before = state(db, "discovery").await;
    let fresh = serde_json::from_str::<Value>(&before).ok().map(|b| b["signature"].clone()) != Some(signature.clone());
    if fresh {
        say("writing the discovered inventory");
        if !before.is_empty() {
            // What is no longer found goes (links and sessions before the
            // devices they name); what is, is written again below.
            for (name, rows) in [("netops_bgp", &found.peers), ("netops_links", &found.links), ("netops_discovered", &found.how), ("netops_devices", &found.devices), ("netops_sites", &found.sites)] {
                let keep: HashSet<&str> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
                let have = db.query(&format!("SELECT id FROM {name} LIMIT 10000"), json!({})).await?;
                let gone: Vec<String> = have.iter().map(|r| r.s("id")).filter(|id| !keep.contains(id.as_str())).collect();
                for part in gone.chunks(500) {
                    let list = part.iter().map(|id| format!("'{}'", id.replace('\'', "''"))).collect::<Vec<_>>().join(", ");
                    db.sql(&format!("DELETE FROM {name} WHERE id IN ({list})")).await?;
                }
            }
        }
        db.upsert("netops_sites", found.sites.clone()).await?;
        db.upsert("netops_devices", found.devices.clone()).await?;
        db.upsert("netops_links", found.links.clone()).await?;
        db.upsert("netops_bgp", found.peers.clone()).await?;
        db.upsert("netops_discovered", found.how.clone()).await?;
    }
    let mut w = Engine::inventory(db).await?.ok_or_else(|| Error { status: 0, message: "discovery wrote no device".into() })?;
    if let Some(t) = &truth {
        graft(&mut w, t);
    }
    let est = if truth_est.is_some() {
        let (plan, how_rows) = estate_plan(&r, &w, &base);
        if fresh {
            let at = iso(now);
            db.upsert("netops_discovered", how_rows.into_iter().map(|mut h| { h["at"] = json!(at); h }).collect()).await?;
        }
        Some(Estate::lay_out(&w, &plan))
    } else {
        None
    };
    let summary = json!({"signature": signature, "at": iso(now), "ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1), "queries": trace.list(),
        "rows": {"snmp_system": r.system.len(), "snmp_ifaces": r.ifaces.len(), "lldp": r.lldp.len(), "bgpls_nodes": r.nodes.len(), "bgpls_links": r.links.len(), "bmp_peers": r.bmp.len(), "gpon_onts": r.onts.len()},
        "found": {"sites": found.sites.len(), "devices": found.devices.len(), "links": found.links.len(), "peers": found.peers.len(), "estate": est.as_ref().map_or(0, |e| e.objects())}});
    db.upsert("netops_state", vec![json!({"id": "discovery", "val": summary.to_string(), "updated": iso(now)}), json!({"id": "topology", "val": "discovered", "updated": iso(now)})]).await?;
    Ok((w, est))
}

// ---- the page

pub fn routes() -> Router<Shared> {
    Router::new().route("/v1/discovery", get(page))
}

/// What the network was read from and how: each feed with its rows, what
/// discovery made of them, and every site, device, link, session and hub
/// with the feeds that found it and the rule.
async fn page(State(svc): State<Shared>) -> Answer {
    let q = Q::new(&svc);
    let last = q.soft("The last discovery", "SELECT val, updated FROM netops_state WHERE id = 'discovery'", json!({})).await;
    let summary: Value = last.first().and_then(|r| serde_json::from_str(&r.s("val")).ok()).unwrap_or(Value::Null);
    let mut feeds = Vec::new();
    for (table, what) in crate::schema::tables().into_iter().filter(|t| crate::schema::FEEDS.contains(&t.0)).map(|t| (t.0, t.2)) {
        let n = q.soft(&format!("Rows in {table}"), &format!("SELECT COUNT(*) AS n FROM {table}"), json!({})).await;
        // Interfaces with a speed: ports, not loopbacks.
        let only = if table == "netops_snmp_ifaces" { " WHERE high_speed > 0" } else { "" };
        let sample = q.soft(&format!("A few rows of {table}"), &format!("SELECT * FROM {table}{only} LIMIT 4"), json!({})).await;
        feeds.push(json!({"table": table, "what": what, "rows": n.first().map_or(0, |r| r.i("n")), "sample": sample}));
    }
    let found = q.rows("How each was found", "SELECT kind, item, sources, how FROM netops_discovered\nWHERE kind IN ('site', 'device', 'link', 'peer')\nORDER BY kind, item LIMIT 2000", json!({})).await?;
    let hubs = q.rows("The estate's hubs, and how each was found", "SELECT kind, item, sources, how FROM netops_discovered\nWHERE kind = 'hub'\nORDER BY item LIMIT 2000", json!({})).await?;
    let mut ran = summary["queries"].as_array().cloned().unwrap_or_default();
    for r in &mut ran {
        r["what"] = json!(format!("Discovery: {}", r["what"].as_str().unwrap_or("")));
    }
    let mut out = q.done(json!({"topology": if svc.engine.opts.discovered { "discovered" } else { "inventory" }, "summary": summary, "feeds": feeds, "found": found, "hubs": hubs}))?;
    if let Some(list) = out.0["queries"].as_array_mut() {
        list.extend(ran);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_of(w: &World, est: Option<&Estate>) -> Read {
        let mut r = Read::of(crate::feeds::backbone(w, 1_800_000_000));
        if let Some(e) = est {
            r.extend(Read::of(crate::feeds::estate(w, e, 0..e.nodes.len(), 1_800_000_000)));
        }
        r
    }

    fn world_of(f: &Found, truth: &World) -> World {
        let rows = |v: &Vec<Value>| v.iter().filter_map(|r| r.as_object().cloned()).collect::<Vec<Row>>();
        let services = rows(&truth.service_rows());
        let customers = rows(&truth.customer_rows());
        let mut w = World::from_rows(&rows(&f.sites), &rows(&f.devices), &rows(&f.links), &services, &customers, &rows(&f.peers));
        graft(&mut w, truth);
        w
    }

    #[test]
    fn locations_are_read_as_operators_write_them() {
        let p = pop_location("AMS | Amsterdam, NL | EU-West | 52.3700,4.9000").unwrap();
        assert_eq!((p.code.as_str(), p.city.as_str(), p.country.as_str(), p.region.as_str()), ("ams", "Amsterdam", "NL", "EU-West"));
        assert!((p.lat - 52.37).abs() < 1e-9 && (p.lon - 4.9).abs() < 1e-9);
        assert_eq!(access_location("AMS | Amsterdam hub 3 | Cabinet 5, Amsterdam hub 3"), Some(("ams".into(), "Amsterdam hub 3".into(), Some("Cabinet 5, Amsterdam hub 3".into()))));
        assert_eq!(vendor_of("1.3.6.1.4.1.2636.1.1.4242"), "Juniper");
        assert_eq!(os_of("Cisco IOS XR Software (ASR 9906), Version 7.9.2"), "IOS XR");
        assert_eq!((first_num("Cabinet 5, Amsterdam hub 3"), last_num("Amsterdam hub 13"), last_num("2/1/12")), (Some(5), Some(13), Some(12)));
    }

    /// The network read back from its feeds is the simulator's: every
    /// device with its role, vendor and software, every cable with its
    /// ports, capacity and plane, and every BGP session. What differs is
    /// what the inventory had wrong: the two cables nobody recorded are
    /// there, and the two records of removed links are not.
    #[test]
    fn the_backbone_is_read_back_from_what_it_reports() {
        let truth = World::build();
        let f = backbone(&read_of(&truth, None), 1_800_000_000);
        let w = world_of(&f, &truth);
        assert_eq!(w.sites.iter().map(|s| (s.id, s.region, s.kind)).collect::<Vec<_>>(), truth.sites.iter().map(|s| (s.id, s.region, s.kind)).collect::<Vec<_>>());
        assert_eq!(w.devices.len(), truth.devices.len());
        for (d, t) in w.devices.iter().zip(&truth.devices) {
            assert_eq!((&d.id, d.num, d.site, d.role, d.vendor, d.model, d.os, &d.os_version, &d.serial, &d.loopback, &d.mgmt_ip), (&t.id, t.num, t.site, t.role, t.vendor, t.model, t.os, &t.os_version, &t.serial, &t.loopback, &t.mgmt_ip));
        }
        let ends = |w: &World, l: &crate::world::Link| {
            let mut e = [(w.devices[l.a].id.clone(), l.a_if.clone()), (w.devices[l.b].id.clone(), l.z_if.clone())];
            e.sort();
            e
        };
        let cabled: BTreeSet<_> = truth.links.iter().filter(|l| l.live).map(|l| (ends(&truth, l), l.kind, l.capacity_mbps, l.plane, l.circuit.clone(), l.provider)).collect();
        let found: BTreeSet<_> = w.links.iter().map(|l| (ends(&w, l), l.kind, l.capacity_mbps, l.plane, l.circuit.clone(), l.provider)).collect();
        assert_eq!(found, cabled);
        // Every backbone link has its other plane, and its delay.
        for l in w.links.iter().filter(|l| l.kind == "backbone") {
            let t = truth.links.iter().find(|t| ends(&truth, t) == ends(&w, l)).unwrap();
            assert!((l.latency_ms - t.latency_ms).abs() < 0.01, "{} {} {}", l.id, l.latency_ms, t.latency_ms);
            let s = l.sibling.expect("a sibling");
            assert_eq!(w.devices[w.links[s].a].site.min(w.devices[w.links[s].b].site), w.devices[l.a].site.min(w.devices[l.b].site));
        }
        assert!(w.link_by_id("cr1-lon--cr1-nyc").is_some() && w.link_by_id("cr1-ams--cr1-fra").is_some());
        let peers = |w: &World| w.peers.iter().map(|p| (p.id.clone(), p.asn, p.name, p.kind, p.prefixes)).collect::<BTreeSet<_>>();
        assert_eq!(peers(&w), peers(&truth));
        assert_eq!(w.customers.len(), truth.customers.len());
        // Every row says how it was found.
        assert_eq!(f.how.len(), f.sites.len() + f.devices.len() + f.links.len() + f.peers.len());
        assert!(f.how.iter().all(|h| !h["how"].as_str().unwrap().is_empty() && !h["sources"].as_array().unwrap().is_empty()));
    }

    /// Discovery reads the feeds, not the simulator: a cable that only the
    /// feeds have is in the network, and a device whose sysDescr says
    /// firewall is one.
    #[test]
    fn what_the_feeds_say_is_what_the_network_is() {
        let truth = World::build();
        let mut r = read_of(&truth, None);
        let (a, b) = (truth.device("lf1-ams").unwrap(), truth.device("lf2-ams").unwrap());
        let mut extra = serde_json::Map::new();
        for (k, v) in [("id", json!("x")), ("agent", json!(a.mgmt_ip)), ("local_port", json!("Ethernet40/1")), ("rem_sys_name", json!(b.id)), ("rem_port", json!("Ethernet40/1"))] {
            extra.insert(k.into(), v);
        }
        r.lldp.push(extra);
        let f = backbone(&r, 1_800_000_000);
        assert!(f.links.iter().any(|l| l["id"] == "lf1-ams--lf2-ams" && l["kind"] == "core"), "the extra cable is a link");
        assert_eq!(f.links.len(), truth.links.iter().filter(|l| l.live).count() + 1);
    }

    /// The estate read back from LLDP, the ONT tables and sysLocation is the
    /// one the simulator made, object for object and place for place.
    #[test]
    fn the_estate_is_read_back_from_what_it_reports() {
        let truth = World::build();
        let est = Estate::build(&truth, 4000);
        let r = read_of(&truth, Some(&est));
        let w = world_of(&backbone(&r, 1_800_000_000), &truth);
        let base: HashMap<String, f64> = est.nodes.iter().map(|n| (n.id.clone(), n.base_ms)).collect();
        let (plan, how_rows) = estate_plan(&r, &w, &base);
        let got = Estate::lay_out(&w, &plan);
        assert_eq!(got.nodes.len(), est.nodes.len());
        for (g, t) in got.nodes.iter().zip(&est.nodes) {
            assert_eq!((&g.id, g.kind, g.layer, g.tier, &g.name, g.anc, g.vendor, g.model, &g.version, &g.customer, g.children), (&t.id, t.kind, t.layer, t.tier, &t.name, t.anc, t.vendor, t.model, &t.version, &t.customer, t.children), "{}", t.id);
            assert!((g.x - t.x).abs() < 1e-9 && (g.y - t.y).abs() < 1e-9 && (g.w - t.w).abs() < 1e-9 && (g.base_ms - t.base_ms).abs() < 1e-9, "{}", t.id);
        }
        // Behind the backbone, the same edges; between its devices, the links discovered.
        let non_core = |e: &Estate| e.edges.iter().filter(|x| x.2 != "backbone" && x.2 != "core").map(|x| (e.nodes[x.0 as usize].id.clone(), e.nodes[x.1 as usize].id.clone(), x.2)).collect::<BTreeSet<_>>();
        assert_eq!(non_core(&got), non_core(&est));
        assert_eq!(how_rows.len(), est.nodes.iter().filter(|n| n.kind == "hub").count());
        assert_eq!((got.hub_r, got.site_r), (est.hub_r, est.site_r));
    }
}
