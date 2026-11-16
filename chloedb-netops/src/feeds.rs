//! What the network says about itself. With `--topology discovered` the
//! simulator keeps its network to itself and writes, instead of an
//! inventory, the rows collectors would write from the network's own
//! protocols; the app then reads the network back from these rows alone
//! (`crate::discover`). Nothing here is read by the app.
//!
//! - BGP-LS (RFC 9552): the IS-IS database as a route reflector hands it
//!   out, a node per router and a link per direction, with the links' TE
//!   attributes: maximum bandwidth, unidirectional delay (RFC 8571) and
//!   administrative group (which plane a backbone link is on).
//! - LLDP-MIB: each device's neighbour table, polled over SNMP.
//! - SNMPv2-MIB, ENTITY-MIB, IF-MIB and IP-MIB: what each box is
//!   (sysDescr, sysObjectID), where it is (sysLocation), its chassis, its
//!   interfaces (with the carriers' circuit ids in their descriptions) and
//!   their addresses.
//! - BMP (RFC 7854): each router's BGP sessions and the routes taken on each.
//! - GPON: each OLT's ONT table, from its management interface.
//!
//! Faithful to what such feeds hold, not more: a power plant appears only
//! in SNMP (no protocol says which routers it feeds), a customer's name only
//! in the description of the switch port they plug into, and LLDP is read
//! from one side of the estate's links (the side a poller asks).

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::estate::{Estate, NONE};
use crate::telemetry::iso;
use crate::world::{Rng, World};

/// IANA's Private Enterprise Numbers of the vendors in the network: what a
/// sysObjectID names after `1.3.6.1.4.1.`.
pub const ENTERPRISES: &[(u32, &str)] = &[
    (9, "Cisco"),
    (193, "Ericsson"),
    (664, "Adtran"),
    (890, "Zyxel"),
    (2636, "Juniper"),
    (6302, "Vertiv"),
    (6321, "Calix"),
    (6527, "Nokia"),
    (12148, "Eltek"),
    (14988, "MikroTik"),
    (25461, "Palo Alto"),
    (30065, "Arista"),
];

/// GPON vendor ids: the first four letters of an ONT's serial number.
pub const GPON_VENDORS: &[(&str, &str)] = &[("ALCL", "Nokia"), ("CXNK", "Calix"), ("ZYXE", "Zyxel")];

/// The administrative group bits of the backbone's two planes.
pub const PLANE_BITS: &[(i64, &str)] = &[(1, "A"), (2, "B")];

/// What a router's own AS is.
pub const LOCAL_ASN: i64 = 64512;

fn pen(vendor: &str) -> u32 {
    ENTERPRISES.iter().find(|e| e.1 == vendor).map_or(8072, |e| e.0)
}

fn hash(s: &str) -> u64 {
    Rng::keyed(s, 0).next()
}

/// A product's sysObjectID: the vendor's enterprise, then a product number.
pub fn sys_object_id(vendor: &str, model: &str) -> String {
    format!("1.3.6.1.4.1.{}.1.1.{}", pen(vendor), hash(model) % 9000 + 1000)
}

/// A device's sysDescr, as its software writes it.
pub fn sys_descr(kind: &str, vendor: &str, model: &str, version: &str) -> String {
    match (vendor, kind) {
        ("Juniper", _) => format!("Juniper Networks, Inc. {} internet router, kernel JUNOS {version}, Build date: 2026-08-14 06:12:41 UTC Copyright (c) 1996-2026 Juniper Networks, Inc.", model.to_lowercase().replace(' ', "")),
        ("Cisco", "cpe") => format!("Cisco IOS XE Software, {} Software (X86_64_LINUX_IOSD-UNIVERSALK9-M), Version {version}, RELEASE SOFTWARE", model.replace(' ', "")),
        ("Cisco", _) => format!("Cisco IOS XR Software ({model}), Version {version} Copyright (c) 2013-2026 by Cisco Systems, Inc."),
        ("Arista", _) => format!("Arista Networks EOS version {version} running on an Arista Networks {model}"),
        ("Palo Alto", _) => format!("Palo Alto Networks {model} series firewall"),
        ("Nokia", "olt") => format!("Nokia {model} GPON OLT, ISAM software {version}"),
        ("Nokia", "cell") => format!("TiMOS-B-{version} both/x86_64 Nokia {model} cell site router Copyright (c) 2000-2026 Nokia."),
        ("Nokia", "radio") => format!("Nokia {model} radio module, software {version}"),
        ("Nokia", _) => format!("TiMOS-C-{version} cpm/x86_64 Nokia {model} Copyright (c) 2000-2026 Nokia."),
        ("Calix", _) => format!("Calix {model} GPON OLT, AXOS {version}"),
        ("Adtran", _) => format!("Adtran {model} GPON OLT, software {version}"),
        ("Ericsson", "cell") => format!("Ericsson {model} cell site router, IPOS {version}"),
        ("Ericsson", _) => format!("Ericsson {model} radio unit, software {version}"),
        ("Eltek", _) => format!("Eltek {model} rectifier system, Smartpack2 controller {version}"),
        ("Vertiv", _) => format!("Vertiv {model} DC power system, controller {version}"),
        ("MikroTik", _) => format!("RouterOS {model} {version}"),
        _ => format!("{vendor} {model} {version}"),
    }
}

/// What LLDP says a device can do.
fn caps(kind: &str) -> Value {
    match kind {
        "firewall" | "cpe" => json!(["router"]),
        "radio" => json!(["other"]),
        _ => json!(["bridge", "router"]),
    }
}

/// A chassis id, locally administered.
fn mac(id: &str) -> String {
    let h = hash(id);
    format!("02:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", (h >> 32) & 255, (h >> 24) & 255, (h >> 16) & 255, (h >> 8) & 255, h & 255)
}

fn loopback_name(vendor: &str) -> &'static str {
    match vendor {
        "Juniper" => "lo0.0",
        "Palo Alto" => "loopback.1",
        _ => "Loopback0",
    }
}

/// An IS-IS system id from a router id, as operators derive it: the four
/// octets padded to three digits and read in groups of four.
fn system_id(ip: &str) -> String {
    let digits: String = ip.split('.').map(|o| format!("{:0>3}", o)).collect();
    if digits.len() != 12 {
        return format!("0000.0000.{:04}", hash(ip) % 10_000);
    }
    format!("{}.{}.{}", &digits[0..4], &digits[4..8], &digits[8..12])
}

/// The rows of each feed.
#[derive(Default)]
pub struct Feeds {
    pub snmp_system: Vec<Value>,
    pub snmp_ifaces: Vec<Value>,
    pub lldp: Vec<Value>,
    pub bgpls_nodes: Vec<Value>,
    pub bgpls_links: Vec<Value>,
    pub bmp_peers: Vec<Value>,
    pub gpon_onts: Vec<Value>,
}

impl Feeds {
    /// Each feed's table and rows.
    pub fn tables(self) -> Vec<(&'static str, Vec<Value>)> {
        vec![
            ("netops_snmp_system", self.snmp_system),
            ("netops_snmp_ifaces", self.snmp_ifaces),
            ("netops_lldp", self.lldp),
            ("netops_bgpls_nodes", self.bgpls_nodes),
            ("netops_bgpls_links", self.bgpls_links),
            ("netops_bmp_peers", self.bmp_peers),
            ("netops_gpon_onts", self.gpon_onts),
        ]
    }

    pub fn len(&self) -> usize {
        self.snmp_system.len() + self.snmp_ifaces.len() + self.lldp.len() + self.bgpls_nodes.len() + self.bgpls_links.len() + self.bmp_peers.len() + self.gpon_onts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Which devices run the IGP: the routers; firewalls and the data centres'
/// fabrics do not.
pub fn in_igp(role: &str) -> bool {
    matches!(role, "core" | "border" | "agg")
}

/// What the backbone's devices report: their system groups, interfaces and
/// LLDP neighbours, the IS-IS database by BGP-LS, and BMP's sessions. Only
/// what is cabled and speaking is reported: an inventory's stale record of
/// a removed link is nowhere here, and a cable nobody recorded is.
pub fn backbone(w: &World, now: i64) -> Feeds {
    let mut f = Feeds::default();
    let at = iso(now);
    let mut if_index: HashMap<usize, i64> = HashMap::new();
    let mut iface = |f: &mut Feeds, d: usize, name: &str, alias: &str, speed: i64, ip: &str, len: i64| {
        let dev = &w.devices[d];
        let n = if_index.entry(d).or_insert(1);
        *n += 1;
        f.snmp_ifaces.push(json!({"id": format!("{}/{}", dev.mgmt_ip, n), "agent": dev.mgmt_ip, "if_index": *n, "if_name": name, "if_alias": alias, "high_speed": speed,
            "oper_status": "up", "ip": ip, "prefix_len": len, "polled": at}));
    };
    for (i, d) in w.devices.iter().enumerate() {
        let s = w.site(d.site);
        f.snmp_system.push(json!({"id": d.mgmt_ip, "sys_name": d.id, "sys_descr": sys_descr(d.role, d.vendor, d.model, &d.os_version), "sys_object_id": sys_object_id(d.vendor, d.model),
            "sys_location": format!("{} | {}, {} | {} | {:.4},{:.4}", s.id.to_uppercase(), s.city, s.country, s.region, s.lat, s.lon),
            "model": d.model, "serial": d.serial, "sw_rev": d.os_version, "polled": at}));
        iface(&mut f, i, loopback_name(d.vendor), "", 0, &d.loopback, 32);
        if in_igp(d.role) {
            f.bgpls_nodes.push(json!({"id": d.loopback, "protocol": "isis-l2", "system_id": system_id(&d.loopback), "hostname": d.id, "area": "49.0001", "received": at}));
        }
    }
    for (k, l) in w.links.iter().enumerate().filter(|(_, l)| l.live) {
        let (a, b) = (&w.devices[l.a], &w.devices[l.b]);
        let routed = in_igp(a.role) && in_igp(b.role);
        let (ip_a, ip_b) = if routed { (format!("172.16.{}.{}", (2 * k) / 256, (2 * k) % 256), format!("172.16.{}.{}", (2 * k + 1) / 256, (2 * k + 1) % 256)) } else { (String::new(), String::new()) };
        let len = if routed { 31 } else { 0 };
        // A leased circuit's interfaces say whose circuit it is.
        let alias = if l.circuit.is_empty() { String::new() } else { format!("{} | {} | plane {}", l.provider, l.circuit, l.plane) };
        iface(&mut f, l.a, &l.a_if, &alias, l.capacity_mbps, &ip_a, len);
        iface(&mut f, l.b, &l.z_if, &alias, l.capacity_mbps, &ip_b, len);
        for (me, port, other, rport) in [(a, &l.a_if, b, &l.z_if), (b, &l.z_if, a, &l.a_if)] {
            f.lldp.push(json!({"id": format!("{}/{port}", me.mgmt_ip), "agent": me.mgmt_ip, "local_port": port, "local_port_desc": "", "rem_chassis": mac(&other.id), "rem_sys_name": other.id,
                "rem_port": rport, "rem_mgmt": other.mgmt_ip, "rem_caps": caps(other.role), "polled": at}));
        }
        if routed {
            let group = PLANE_BITS.iter().find(|p| p.1 == l.plane).map_or(0, |p| p.0);
            let metric = if l.kind == "backbone" { (l.latency_ms * 10.0) as i64 } else { 10 };
            for (me, my_ip, other, their_ip) in [(a, &ip_a, b, &ip_b), (b, &ip_b, a, &ip_a)] {
                f.bgpls_links.push(json!({"id": format!("{}>{}:{my_ip}", me.loopback, other.loopback), "local_node": me.loopback, "remote_node": other.loopback, "local_ip": my_ip, "remote_ip": their_ip,
                    "igp_metric": metric, "max_bw_mbps": l.capacity_mbps, "delay_us": (l.latency_ms * 1000.0).round() as i64, "admin_group": group, "received": at}));
            }
        }
    }
    for p in &w.peers {
        let d = &w.devices[p.device];
        f.bmp_peers.push(json!({"id": format!("{}/{}", d.loopback, p.peer_ip), "router": d.loopback, "local_asn": LOCAL_ASN, "peer_ip": p.peer_ip, "peer_asn": p.asn, "peer_bgp_id": p.peer_ip,
            "peer_desc": p.name, "state": "up", "adj_rib_in": p.prefixes, "up_since": iso(now - 23 * 86400)}));
    }
    f
}

/// An estate object's management address.
pub fn estate_mgmt(i: u32) -> String {
    format!("100.{}.{}.{}", 64 + (i >> 16), (i >> 8) & 255, i & 255)
}

/// What the objects `range` of the estate report: their system groups
/// (sysLocation naming the metro, the hub and the site), the LLDP
/// neighbours a poller reads from each (a hub router's toward the
/// backbone, an access node's toward its hub routers and the radios and
/// customer routers behind it), and each OLT's ONT table.
pub fn estate(w: &World, est: &Estate, range: std::ops::Range<usize>, now: i64) -> Feeds {
    let mut f = Feeds::default();
    let at = iso(now);
    let mgmt_of = |i: u32| -> String {
        let n = &est.nodes[i as usize];
        if n.layer == "core" {
            w.device(&n.id).map(|d| d.mgmt_ip.clone()).unwrap_or_default()
        } else {
            estate_mgmt(i)
        }
    };
    for i in range.start as u32..range.end.min(est.nodes.len()) as u32 {
        let n = &est.nodes[i as usize];
        if matches!(n.layer, "container" | "core") {
            continue;
        }
        let name_of = |a: u32| if a == NONE { String::new() } else { est.nodes[a as usize].name.clone() };
        let code = if n.anc[1] == NONE { String::new() } else { est.nodes[n.anc[1] as usize].id.trim_start_matches("m-").to_uppercase() };
        let location = match n.anc[3] {
            NONE => format!("{code} | {}", name_of(n.anc[2])),
            s => format!("{code} | {} | {}", name_of(n.anc[2]), name_of(s)),
        };
        let me = estate_mgmt(i);
        if n.kind != "ont" {
            let serial = format!("{}{:08X}", &n.vendor[..2].to_uppercase(), hash(&n.id) as u32);
            f.snmp_system.push(json!({"id": me, "sys_name": n.id, "sys_descr": sys_descr(n.kind, n.vendor, n.model, &n.version), "sys_object_id": sys_object_id(n.vendor, n.model),
                "sys_location": location, "model": n.model, "serial": serial, "sw_rev": n.version, "polled": at}));
        }
        let lldp = |f: &mut Feeds, port: String, desc: String, other: u32, rport: String| {
            let o = &est.nodes[other as usize];
            f.lldp.push(json!({"id": format!("{me}/{port}"), "agent": me, "local_port": port, "local_port_desc": desc, "rem_chassis": mac(&o.id), "rem_sys_name": o.id,
                "rem_port": rport, "rem_mgmt": mgmt_of(other), "rem_caps": caps(o.kind), "polled": at}));
        };
        match n.layer {
            // Toward what it hangs from: the backbone, or its hub's routers.
            "aggregation" | "access" => {
                for (k, &up) in est.ups(i).iter().enumerate().filter(|(_, &u)| est.nodes[u as usize].layer != "power") {
                    lldp(&mut f, format!("1/1/{}", k + 1), String::new(), up, format!("1/2/{}", 1 + (i as usize % 40)));
                }
            }
            _ => {}
        }
        if n.layer == "access" {
            for (b, &down) in est.downs(i).iter().enumerate() {
                let o = &est.nodes[down as usize];
                if o.kind == "ont" {
                    let vid = GPON_VENDORS.iter().find(|g| g.1 == o.vendor).map_or("GPON", |g| g.0);
                    f.gpon_onts.push(json!({"id": format!("{me}/{}/{}", 1 + b / 32, b % 32 + 1), "agent": me, "pon_port": format!("1/1/{}", 1 + b / 32), "ont_id": b % 32 + 1,
                        "serial": format!("{vid}{:08X}", hash(&o.id) as u32), "model": o.model, "version": o.version, "description": o.id, "oper_status": "up"}));
                } else {
                    let desc = if o.customer.is_empty() { String::new() } else { format!("CUST {}", o.customer) };
                    lldp(&mut f, format!("2/1/{}", b + 1), desc, down, "1".into());
                }
            }
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_isis_system_id_is_read_off_the_router_id() {
        assert_eq!(system_id("192.0.2.12"), "1920.0000.2012");
        assert_eq!(system_id("10.1.1.1"), "0100.0100.1001");
    }

    #[test]
    fn the_backbone_reports_what_is_cabled_and_nothing_else() {
        let w = World::build();
        let f = backbone(&w, 1_800_000_000);
        assert_eq!(f.snmp_system.len(), w.devices.len());
        let live = w.links.iter().filter(|l| l.live).count();
        assert_eq!(f.lldp.len(), 2 * live, "each cable from both ends");
        // A stale record is in no feed.
        assert!(!f.snmp_ifaces.iter().any(|r| r["if_alias"].as_str().unwrap_or("").contains("TEL-STO-WAW-0412")));
        // The routers are in the IS-IS database; the fabric and the firewalls are not.
        assert_eq!(f.bgpls_nodes.len(), w.devices.iter().filter(|d| in_igp(d.role)).count());
        assert!(f.bgpls_links.len() % 2 == 0 && !f.bgpls_links.is_empty());
        assert_eq!(f.bmp_peers.len(), w.peers.len());
    }
}
