//! The network the app shows: a made-up carrier, "Meridian", with ten sites
//! in Europe and North America, 64 devices, their links, the services that
//! ride on them and the customers who buy those. It is built the same way
//! every time (no randomness that a seed does not fix), so the simulator
//! can rebuild it after a restart and tests can name its devices.
//!
//! Everything here is synthetic. The app says so on every screen.

use serde_json::{json, Value};

use crate::strata::{Row, RowExt};

/// A small deterministic generator (splitmix64), so the crate needs no
/// random-number dependency and every run makes the same network.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    /// A generator for one named thing at one moment: the same name and
    /// number always give the same stream.
    pub fn keyed(name: &str, n: u64) -> Rng {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in name.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        Rng::new(h ^ n.wrapping_mul(0xD6E8_FEB8_6659_FD93))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.unit()
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    /// Roughly normal, mean 0 and deviation 1.
    pub fn normal(&mut self) -> f64 {
        (0..6).map(|_| self.unit()).sum::<f64>() * 2.0_f64.sqrt() - 3.0 * 2.0_f64.sqrt()
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

#[derive(Clone, Debug)]
pub struct Site {
    pub id: &'static str,
    pub name: &'static str,
    pub city: &'static str,
    pub country: &'static str,
    pub region: &'static str,
    pub lat: f64,
    pub lon: f64,
    /// `dc` sites have a leaf-spine fabric and firewalls; `pop` sites only route.
    pub kind: &'static str,
    /// Hours east of UTC, for the daily traffic curve.
    pub tz: f64,
    /// How much traffic the site draws, 1.0 being the largest.
    pub weight: f64,
}

/// A name read from a table, kept for the life of the process (the model's
/// names are `&'static str`, as the simulator's are constants). Each distinct
/// name is kept once.
fn keep(s: &str) -> &'static str {
    static KEPT: std::sync::Mutex<Option<std::collections::HashSet<&'static str>>> = std::sync::Mutex::new(None);
    let mut g = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let set = g.get_or_insert_with(Default::default);
    match set.get(s) {
        Some(k) => k,
        None => {
            let k: &'static str = Box::leak(s.to_string().into_boxed_str());
            set.insert(k);
            k
        }
    }
}

pub const SITES: &[Site] = &[
    Site { id: "ams", name: "Amsterdam", city: "Amsterdam", country: "NL", region: "EU-West", lat: 52.37, lon: 4.90, kind: "dc", tz: 1.0, weight: 1.0 },
    Site { id: "fra", name: "Frankfurt", city: "Frankfurt", country: "DE", region: "EU-Central", lat: 50.11, lon: 8.68, kind: "dc", tz: 1.0, weight: 0.95 },
    Site { id: "lon", name: "London", city: "London", country: "GB", region: "EU-West", lat: 51.51, lon: -0.13, kind: "hub", tz: 0.0, weight: 0.9 },
    Site { id: "par", name: "Paris", city: "Paris", country: "FR", region: "EU-West", lat: 48.86, lon: 2.35, kind: "pop", tz: 1.0, weight: 0.7 },
    Site { id: "mad", name: "Madrid", city: "Madrid", country: "ES", region: "EU-South", lat: 40.42, lon: -3.70, kind: "pop", tz: 1.0, weight: 0.5 },
    Site { id: "mil", name: "Milan", city: "Milan", country: "IT", region: "EU-South", lat: 45.46, lon: 9.19, kind: "pop", tz: 1.0, weight: 0.55 },
    Site { id: "sto", name: "Stockholm", city: "Stockholm", country: "SE", region: "EU-North", lat: 59.33, lon: 18.07, kind: "pop", tz: 1.0, weight: 0.4 },
    Site { id: "waw", name: "Warsaw", city: "Warsaw", country: "PL", region: "EU-Central", lat: 52.23, lon: 21.01, kind: "pop", tz: 1.0, weight: 0.45 },
    Site { id: "nyc", name: "New York", city: "New York", country: "US", region: "US-East", lat: 40.71, lon: -74.01, kind: "hub", tz: -5.0, weight: 0.85 },
    Site { id: "ash", name: "Ashburn", city: "Ashburn", country: "US", region: "US-East", lat: 39.04, lon: -77.49, kind: "pop", tz: -5.0, weight: 0.6 },
];

#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub num: i64,
    pub site: &'static str,
    pub role: &'static str,
    pub vendor: &'static str,
    pub model: &'static str,
    pub os: &'static str,
    pub os_version: String,
    pub mgmt_ip: String,
    pub loopback: String,
    pub serial: String,
    pub tags: Vec<&'static str>,
    /// Interfaces handed out so far (for naming the next one).
    ports: usize,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub id: String,
    pub num: i64,
    pub a: usize,
    pub b: usize,
    pub a_if: String,
    pub z_if: String,
    /// backbone (between sites), core (inside a site), uplink (to the fabric
    /// or the firewalls), fabric (spine to leaf).
    pub kind: &'static str,
    pub capacity_mbps: i64,
    pub circuit: String,
    pub provider: &'static str,
    /// "A" or "B": each backbone path exists twice, once per plane.
    pub plane: &'static str,
    /// The link that carries this one's traffic when it is down.
    pub sibling: Option<usize>,
    /// The share of capacity used at the daily peak with no fault.
    pub load: f64,
    pub latency_ms: f64,
    /// In the inventory (the NetBox the app pretends to follow).
    pub documented: bool,
    /// Cabled and speaking (LLDP, adjacencies): false for an inventory
    /// record of a link that was removed and never cleaned up.
    pub live: bool,
}

#[derive(Clone, Debug)]
pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    pub tier: i64,
    pub owner: &'static str,
    pub kind: &'static str,
    pub description: &'static str,
    pub slo: f64,
    pub latency_slo_ms: f64,
    pub revenue_per_min: f64,
    /// Requests a second at the daily peak.
    pub rps: f64,
    pub base_latency_ms: f64,
    pub devices: Vec<usize>,
    pub sites: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct Customer {
    pub id: String,
    pub num: i64,
    pub name: String,
    pub tier: &'static str,
    pub site: &'static str,
    pub device: usize,
    pub services: Vec<&'static str>,
    pub mrr: f64,
    /// The addresses routed to them, the port they attach on, its VLAN and
    /// the access circuit's id: what a ticket or a phone call names.
    pub prefix: String,
    pub port: String,
    pub vlan: i64,
    pub circuit: String,
}

#[derive(Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub device: usize,
    pub peer_ip: String,
    pub asn: i64,
    pub name: &'static str,
    pub kind: &'static str,
    pub prefixes: i64,
}

pub struct World {
    pub sites: Vec<Site>,
    pub devices: Vec<Device>,
    pub links: Vec<Link>,
    pub services: Vec<Service>,
    pub customers: Vec<Customer>,
    pub peers: Vec<Peer>,
}

/// Role, vendor, model, operating system and the versions in the fleet (the
/// last one is the current release; older ones are what advisories match).
const KINDS: &[(&str, &str, &str, &str, &[&str])] = &[
    ("core", "Juniper", "MX10008", "Junos", &["22.4R3-S2", "22.4R3-S2", "22.2R3-S1", "23.2R2"]),
    ("border", "Cisco", "ASR 9906", "IOS XR", &["7.9.2", "7.9.2", "7.7.21", "7.10.1"]),
    ("agg", "Arista", "7280R3", "EOS", &["4.31.2F", "4.31.2F", "4.29.4M", "4.30.5M"]),
    ("spine", "Arista", "7060X5", "EOS", &["4.31.2F", "4.30.5M"]),
    ("leaf", "Arista", "7050X3", "EOS", &["4.31.2F", "4.31.2F", "4.29.4M"]),
    ("firewall", "Palo Alto", "PA-5450", "PAN-OS", &["11.0.3-h1", "10.2.7"]),
];

fn port_name(vendor: &str, n: usize) -> String {
    match vendor {
        "Juniper" => format!("et-0/{}/{}", n / 8, n % 8),
        "Cisco" => format!("HundredGigE0/{}/0/{}", n / 12, n % 12),
        "Arista" => format!("Ethernet{}/1", n + 1),
        _ => format!("ethernet1/{}", n + 1),
    }
}

const PROVIDERS: &[&str] = &["EuroFiber", "Colt", "Zayo", "Lumen", "Telia Carrier", "GTT"];

/// Backbone paths between sites: (a, z, capacity in Gb/s, provider index,
/// peak load, latency in ms).
const BACKBONE: &[(&str, &str, i64, usize, f64, f64)] = &[
    ("ams", "fra", 400, 0, 0.58, 6.2),
    ("ams", "lon", 400, 1, 0.55, 7.4),
    ("lon", "par", 200, 1, 0.52, 7.9),
    ("par", "fra", 200, 0, 0.50, 9.6),
    ("par", "mad", 100, 4, 0.46, 18.3),
    ("mad", "mil", 100, 5, 0.38, 21.7),
    ("mil", "fra", 200, 2, 0.47, 10.8),
    ("fra", "waw", 100, 4, 0.49, 15.1),
    ("waw", "sto", 100, 4, 0.33, 16.4),
    ("sto", "ams", 100, 4, 0.44, 19.8),
    ("lon", "nyc", 400, 3, 0.57, 68.5),
    ("ams", "nyc", 200, 2, 0.48, 74.9),
    ("nyc", "ash", 400, 3, 0.51, 5.6),
    ("ash", "par", 100, 5, 0.42, 79.2),
];

const SERVICES: &[(&str, &str, i64, &str, &str, &str, f64, f64, f64, f64, f64, &[&str])] = &[
    ("ip-transit", "IP Transit", 1, "Backbone", "connectivity", "Internet transit for carriers and large enterprises, across the whole backbone", 0.9995, 80.0, 4200.0, 52000.0, 24.0, &["ams", "fra", "lon", "nyc"]),
    ("l3vpn", "MPLS L3VPN", 1, "Backbone", "connectivity", "Private routed networks between customer sites", 0.9995, 60.0, 3100.0, 18000.0, 19.0, &["ams", "fra", "lon", "par", "mad", "mil"]),
    ("wave", "Wavelength Transport", 1, "Optical", "connectivity", "Dedicated 100G waves between hubs", 0.9999, 40.0, 2600.0, 900.0, 9.0, &["ams", "fra", "lon"]),
    ("cloud-connect", "Cloud Connect", 1, "Interconnect", "connectivity", "Private paths into AWS, Azure and Google Cloud from Frankfurt, Amsterdam and Ashburn", 0.9995, 35.0, 2900.0, 14000.0, 11.0, &["fra", "ams", "ash"]),
    ("ddos-shield", "DDoS Shield", 2, "Security", "security", "Scrubbing for attacked prefixes, in Amsterdam and New York", 0.999, 120.0, 1400.0, 6000.0, 31.0, &["ams", "nyc"]),
    ("dns", "Recursive DNS", 2, "Platform", "platform", "Anycast resolvers for access customers", 0.9999, 25.0, 600.0, 88000.0, 6.0, &["ams", "fra", "lon", "nyc"]),
    ("cdn-edge", "CDN Edge", 2, "Platform", "platform", "Cache nodes in the two data centres", 0.999, 90.0, 1900.0, 41000.0, 28.0, &["ams", "fra"]),
    ("voice", "SIP Trunking", 2, "Voice", "voice", "Voice trunks for contact centres; sensitive to loss and jitter", 0.9995, 150.0, 1700.0, 2400.0, 42.0, &["lon", "par", "mad"]),
    ("sdwan", "Managed SD-WAN", 3, "Enterprise", "managed", "Overlay hubs for branch networks", 0.999, 110.0, 1200.0, 7600.0, 37.0, &["lon", "fra", "waw", "sto"]),
    ("ix-peering", "Peering at AMS-IX and DE-CIX", 2, "Interconnect", "connectivity", "Settlement-free peering ports", 0.999, 30.0, 800.0, 36000.0, 4.0, &["ams", "fra"]),
    ("portal", "Customer Portal", 3, "Digital", "application", "Ordering, tickets and usage graphs for customers; hosted in Amsterdam", 0.995, 400.0, 300.0, 420.0, 130.0, &["ams"]),
    ("payments-api", "Billing and Payments API", 2, "Digital", "application", "Invoices and card payments; hosted in Frankfurt behind the firewalls", 0.9995, 250.0, 2100.0, 310.0, 85.0, &["fra"]),
];

const PEERS: &[(i64, &str, &str, i64)] = &[
    (3356, "Lumen", "transit", 940_000),
    (1299, "Arelion", "transit", 935_000),
    (174, "Cogent", "transit", 928_000),
    (15169, "Google", "peer", 1_150),
    (32934, "Meta", "peer", 620),
    (16509, "Amazon", "peer", 7_800),
    (8075, "Microsoft", "peer", 1_020),
    (13335, "Cloudflare", "peer", 3_900),
    (20940, "Akamai", "peer", 9_400),
    (2906, "Netflix", "peer", 310),
    (6939, "Hurricane Electric", "peer", 210_000),
    (714, "Apple", "peer", 1_480),
];

const COMPANY_A: &[&str] = &["Nordlys", "Altamira", "Kestrel", "Brightwater", "Ostrava", "Halcyon", "Verano", "Tidewell", "Corvid", "Lumière", "Sable", "Ironbridge", "Aurelia", "Fjordline", "Quillon", "Marlow", "Cobalt", "Windrose", "Peregrine", "Solent"];
const COMPANY_B: &[&str] = &["Bank", "Logistics", "Media", "Health", "Retail", "Energy", "Insurance", "Games", "Airlines", "Telecom", "Pharma", "Foods", "Motors", "Labs", "Mutual", "Freight"];

impl World {
    pub fn build() -> World {
        let mut w = World { sites: SITES.to_vec(), devices: Vec::new(), links: Vec::new(), services: Vec::new(), customers: Vec::new(), peers: Vec::new() };
        let mut rng = Rng::new(20_261_113);
        // Devices: two cores, borders and aggregation routers per site; a
        // fabric and a firewall pair in the data centres.
        for (si, site) in SITES.iter().enumerate() {
            let add = |w: &mut World, role: &'static str, n: usize, rng: &mut Rng| {
                let (_, vendor, model, os, versions) = KINDS.iter().find(|k| k.0 == role).expect("a role");
                let short = match role { "core" => "cr", "border" => "br", "agg" => "ag", "spine" => "sp", "leaf" => "lf", _ => "fw" };
                let num = w.devices.len() as i64 + 1;
                let version = versions[(num as usize * 7 + n) % versions.len()].to_string();
                w.devices.push(Device {
                    id: format!("{short}{n}-{}", site.id),
                    num,
                    site: site.id,
                    role,
                    vendor,
                    model,
                    os,
                    os_version: version,
                    mgmt_ip: format!("10.{}.0.{}", 10 + si, 10 + w.devices.iter().filter(|d| d.site == site.id).count()),
                    loopback: format!("192.0.2.{num}"),
                    serial: format!("{}{:08X}", &vendor[..2].to_uppercase(), rng.next() as u32),
                    tags: match role { "core" => vec!["backbone", "mpls"], "border" => vec!["edge", "bgp"], "agg" => vec!["access", "customers"], "firewall" => vec!["security"], _ => vec!["fabric"] },
                    ports: 0,
                });
            };
            add(&mut w, "core", 1, &mut rng);
            add(&mut w, "core", 2, &mut rng);
            add(&mut w, "border", 1, &mut rng);
            if site.kind != "pop" {
                add(&mut w, "border", 2, &mut rng);
            }
            add(&mut w, "agg", 1, &mut rng);
            if site.kind != "pop" {
                add(&mut w, "agg", 2, &mut rng);
            }
            if site.kind == "dc" {
                add(&mut w, "firewall", 1, &mut rng);
                add(&mut w, "firewall", 2, &mut rng);
                add(&mut w, "spine", 1, &mut rng);
                add(&mut w, "spine", 2, &mut rng);
                for n in 1..=4 {
                    add(&mut w, "leaf", n, &mut rng);
                }
            }
        }
        // Links inside each site.
        for site in SITES {
            let of = |w: &World, role: &str| -> Vec<usize> { w.devices.iter().enumerate().filter(|(_, d)| d.site == site.id && d.role == role).map(|(i, _)| i).collect() };
            let (cores, borders, aggs, fws, spines, leaves) = (of(&w, "core"), of(&w, "border"), of(&w, "agg"), of(&w, "firewall"), of(&w, "spine"), of(&w, "leaf"));
            w.link(cores[0], cores[1], "core", 400, 0.30, 0.2, "", "");
            for (i, &b) in borders.iter().enumerate() {
                for (j, &c) in cores.iter().enumerate() {
                    w.link(b, c, "core", 100, if i == j % borders.len() { 0.52 } else { 0.34 }, 0.2, "", "");
                }
            }
            for (i, &a) in aggs.iter().enumerate() {
                for (j, &c) in cores.iter().enumerate() {
                    w.link(a, c, "uplink", 100, if i == j % aggs.len() { 0.47 } else { 0.31 }, 0.2, "", "");
                }
            }
            for (i, &f) in fws.iter().enumerate() {
                w.link(f, cores[i % cores.len()], "uplink", 100, 0.36, 0.2, "", "");
                w.link(f, spines[i % spines.len()], "uplink", 100, 0.36, 0.1, "", "");
            }
            for &s in &spines {
                for (k, &l) in leaves.iter().enumerate() {
                    w.link(s, l, "fabric", 100, 0.28 + 0.04 * k as f64, 0.1, "", "");
                }
            }
        }
        // The backbone, twice: plane A between the first cores, plane B
        // between the second ones.
        for &(a, z, gbps, prov, load, ms) in BACKBONE {
            let mut pair = [0usize; 2];
            for (p, plane) in ["A", "B"].iter().enumerate() {
                let ca = w.dev(&format!("cr{}-{a}", p + 1));
                let cz = w.dev(&format!("cr{}-{z}", p + 1));
                let provider = PROVIDERS[(prov + p) % PROVIDERS.len()];
                let circuit = format!("{}-{}-{}-{:04}", &provider[..3].to_uppercase(), a.to_uppercase(), z.to_uppercase(), 1000 + w.links.len() * 7 % 9000);
                pair[p] = w.link(ca, cz, "backbone", gbps * 1000, load + if p == 0 { 0.03 } else { -0.03 }, ms + p as f64 * 0.7, &circuit, provider);
                w.links[pair[p]].plane = plane;
            }
            w.links[pair[0]].sibling = Some(pair[1]);
            w.links[pair[1]].sibling = Some(pair[0]);
        }
        // What the inventory gets wrong, for the topology read from logs:
        // two cables nobody recorded, and two records of links long gone.
        let x = w.link(w.dev("ag1-lon"), w.dev("ag2-lon"), "core", 100, 0.12, 0.2, "", "");
        w.links[x].documented = false;
        let x = w.link(w.dev("br1-ams"), w.dev("br2-ams"), "core", 100, 0.09, 0.2, "", "");
        w.links[x].documented = false;
        let x = w.link(w.dev("ag1-par"), w.dev("br1-par"), "core", 40, 0.0, 0.2, "", "");
        w.links[x].live = false;
        let x = w.link(w.dev("cr1-sto"), w.dev("cr2-waw"), "backbone", 10_000, 0.0, 17.0, "TEL-STO-WAW-0412", "Telia Carrier");
        w.links[x].live = false;
        // Sessions with other networks, on the borders.
        let borders: Vec<usize> = w.devices.iter().enumerate().filter(|(_, d)| d.role == "border").map(|(i, _)| i).collect();
        for (bi, &b) in borders.iter().enumerate() {
            let n = if w.devices[b].site == "ams" || w.devices[b].site == "fra" || w.devices[b].site == "lon" || w.devices[b].site == "nyc" { 5 } else { 3 };
            for k in 0..n {
                let (asn, name, kind, prefixes) = PEERS[(bi * 3 + k * 5) % PEERS.len()];
                if w.peers.iter().any(|p| p.device == b && p.asn == asn) {
                    continue;
                }
                w.peers.push(Peer { id: format!("{}:as{asn}", w.devices[b].id), device: b, peer_ip: format!("198.51.{}.{}", 100 + bi, 1 + k * 4), asn, name, kind, prefixes });
            }
        }
        // Services, each on the cores, borders and aggregation routers of
        // its sites (and the fabric, for what is hosted).
        for &(id, name, tier, owner, kind, description, slo, lat, rev, rps, base, sites) in SERVICES {
            let hosted = kind == "application" || id == "cdn-edge";
            let devices: Vec<usize> = w
                .devices
                .iter()
                .enumerate()
                .filter(|(_, d)| sites.contains(&d.site))
                .filter(|(_, d)| match d.role {
                    "core" => true,
                    "border" => kind != "application" && id != "l3vpn" && id != "wave",
                    "agg" => kind != "application" && id != "ix-peering" && id != "wave",
                    _ => hosted,
                })
                .map(|(i, _)| i)
                .collect();
            w.services.push(Service { id, name, tier, owner, kind, description, slo, latency_slo_ms: lat, revenue_per_min: rev, rps, base_latency_ms: base, devices, sites: sites.to_vec() });
        }
        // Customers, attached to an aggregation router of their site.
        let aggs: Vec<usize> = w.devices.iter().enumerate().filter(|(_, d)| d.role == "agg").map(|(i, _)| i).collect();
        for n in 0..48 {
            let name = format!("{} {}", COMPANY_A[n % COMPANY_A.len()], COMPANY_B[(n * 7 + n / COMPANY_A.len()) % COMPANY_B.len()]);
            let device = aggs[(n * 5 + n / 3) % aggs.len()];
            let site = w.devices[device].site;
            let tier = match n % 6 { 0 => "platinum", 1 | 2 => "gold", _ => "silver" };
            let mut services: Vec<&'static str> = w.services.iter().filter(|s| s.sites.contains(&site) && s.kind != "application").map(|s| s.id).collect();
            let k = n % services.len().max(1);
            services.rotate_left(k);
            services.truncate(1 + n % 3);
            let mrr = match tier { "platinum" => 84_000.0, "gold" => 31_000.0, _ => 9_500.0 } * (0.7 + rng.unit() * 0.8);
            let d = &w.devices[device];
            w.customers.push(Customer {
                id: format!("cust-{:03}", n + 1), num: n as i64 + 1, name, tier, site, device, services, mrr: (mrr / 100.0).round() * 100.0,
                prefix: format!("185.{}.{}.0/24", 60 + SITES.iter().position(|s| s.id == site).unwrap_or(0), n + 1),
                port: port_name(d.vendor, 24 + n % 20), vlan: 1000 + n as i64 + 1, circuit: format!("ACC-{}-{:04}", site.to_uppercase(), 4100 + n * 7),
            });
        }
        w
    }

    pub fn dev(&self, id: &str) -> usize {
        self.devices.iter().position(|d| d.id == id).unwrap_or_else(|| panic!("no device {id}"))
    }

    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn link_by_id(&self, id: &str) -> Option<usize> {
        self.links.iter().position(|l| l.id == id)
    }

    #[allow(clippy::too_many_arguments)]
    fn link(&mut self, a: usize, b: usize, kind: &'static str, capacity_gbps_or_mbps: i64, load: f64, latency_ms: f64, circuit: &str, provider: &'static str) -> usize {
        let capacity_mbps = if capacity_gbps_or_mbps < 1000 { capacity_gbps_or_mbps * 1000 } else { capacity_gbps_or_mbps };
        let a_if = port_name(self.devices[a].vendor, self.devices[a].ports);
        let z_if = port_name(self.devices[b].vendor, self.devices[b].ports);
        self.devices[a].ports += 1;
        self.devices[b].ports += 1;
        let id = format!("{}--{}", self.devices[a].id, self.devices[b].id);
        self.links.push(Link { id, num: self.links.len() as i64 + 1, a, b, a_if, z_if, kind, capacity_mbps, circuit: circuit.to_string(), provider, plane: "", sibling: None, load, latency_ms, documented: true, live: true });
        self.links.len() - 1
    }

    /// The links of a device.
    pub fn links_of(&self, d: usize) -> Vec<usize> {
        self.links.iter().enumerate().filter(|(_, l)| l.a == d || l.b == d).map(|(i, _)| i).collect()
    }

    pub fn site(&self, id: &str) -> &Site {
        self.sites.iter().find(|s| s.id == id).unwrap_or(&SITES[0])
    }

    // ---- a network that is not the simulator's

    /// The network as the inventory tables describe it: what the app works
    /// from when the simulator is off and the tables are fed by real sources.
    /// Rows that name a device the inventory does not have are left out.
    pub fn from_rows(sites: &[Row], devices: &[Row], links: &[Row], services: &[Row], customers: &[Row], peers: &[Row]) -> World {
        let sites: Vec<Site> = sites.iter().map(|r| Site { id: keep(&r.s("id")), name: keep(&r.s("name")), city: keep(&r.s("city")), country: keep(&r.s("country")), region: keep(&r.s("region")),
            lat: r.f("lat"), lon: r.f("lon"), kind: keep(&r.s("kind")), tz: (r.f("lon") / 15.0).round(), weight: 1.0 }).collect();
        let mut rows: Vec<&Row> = devices.iter().collect();
        rows.sort_by_key(|r| r.i("num"));
        let devices: Vec<Device> = rows.iter().map(|r| Device { id: r.s("id"), num: r.i("num"), site: keep(&r.s("site")), role: keep(&r.s("role")), vendor: keep(&r.s("vendor")), model: keep(&r.s("model")), os: keep(&r.s("os")),
            os_version: r.s("os_version"), mgmt_ip: r.s("mgmt_ip"), loopback: r.s("loopback"), serial: r.s("serial"), tags: r.list("tags").iter().map(|t| keep(t)).collect(), ports: 0 }).collect();
        let at = |id: &str| devices.iter().position(|d| d.id == id);
        let mut out: Vec<Link> = Vec::new();
        for r in links {
            let (Some(a), Some(b)) = (at(&r.s("src")), at(&r.s("dst"))) else { continue };
            out.push(Link { id: r.s("id"), num: r.i("num"), a, b, a_if: r.s("a_if"), z_if: r.s("z_if"), kind: keep(&r.s("kind")), capacity_mbps: r.i("capacity_mbps").max(1), circuit: r.s("circuit"), provider: keep(&r.s("provider")),
                plane: keep(&r.s("plane")), sibling: None, load: 0.5, latency_ms: r.f("latency_ms"), documented: true, live: true });
        }
        // A link's sibling is the same path on the other plane.
        for i in 0..out.len() {
            if out[i].plane.is_empty() {
                continue;
            }
            let ends = |l: &Link| { let mut e = [devices[l.a].site, devices[l.b].site]; e.sort(); e };
            out[i].sibling = (0..out.len()).find(|&j| j != i && out[j].kind == out[i].kind && out[j].plane != out[i].plane && !out[j].plane.is_empty() && ends(&out[j]) == ends(&out[i]));
        }
        let services = services.iter().map(|r| Service { id: keep(&r.s("id")), name: keep(&r.s("name")), tier: r.i("tier"), owner: keep(&r.s("owner")), kind: keep(&r.s("kind")), description: keep(&r.s("description")),
            slo: r.f("slo_target"), latency_slo_ms: r.f("latency_slo_ms"), revenue_per_min: r.f("revenue_per_min"), rps: 0.0, base_latency_ms: 0.0,
            devices: r.list("devices").iter().filter_map(|d| at(d)).collect(), sites: r.list("sites").iter().map(|x| keep(x)).collect() }).collect();
        let customers = customers.iter().filter_map(|r| Some(Customer { id: r.s("id"), num: r.i("num"), name: r.s("name"), tier: keep(&r.s("tier")), site: keep(&r.s("site")), device: at(&r.s("device"))?,
            services: r.list("services").iter().map(|x| keep(x)).collect(), mrr: r.f("mrr"), prefix: r.s("prefix"), port: r.s("port"), vlan: r.i("vlan"), circuit: r.s("circuit") })).collect();
        let peers = peers.iter().filter_map(|r| Some(Peer { id: r.s("id"), device: at(&r.s("device"))?, peer_ip: r.s("peer_ip"), asn: r.i("peer_asn"), name: keep(&r.s("peer_name")), kind: keep(&r.s("kind")), prefixes: r.i("prefixes_rx") })).collect();
        World { sites, devices, links: out, services, customers, peers }
    }

    // ---- rows

    pub fn site_rows(&self) -> Vec<Value> {
        self.sites.iter().map(|s| json!({"id": s.id, "name": s.name, "city": s.city, "country": s.country, "region": s.region, "kind": s.kind, "lat": s.lat, "lon": s.lon,
            "devices": self.devices.iter().filter(|d| d.site == s.id).count()})).collect()
    }

    pub fn device_rows(&self, installed: &str) -> Vec<Value> {
        self.devices
            .iter()
            .map(|d| json!({"id": d.id, "num": d.num, "site": d.site, "role": d.role, "vendor": d.vendor, "model": d.model, "os": d.os, "os_version": d.os_version,
                "mgmt_ip": d.mgmt_ip, "loopback": d.loopback, "asn": 64512, "status": "up", "serial": d.serial, "tags": d.tags, "installed": installed}))
            .collect()
    }

    /// The inventory's links: every documented one, live or not.
    pub fn link_rows(&self) -> Vec<Value> {
        self.links
            .iter()
            .filter(|l| l.documented)
            .map(|l| json!({"id": l.id, "src": self.devices[l.a].id, "dst": self.devices[l.b].id, "num": l.num, "kind": l.kind, "a_if": l.a_if, "z_if": l.z_if,
                "capacity_mbps": l.capacity_mbps, "circuit": l.circuit, "provider": l.provider, "plane": l.plane, "status": "up", "latency_ms": l.latency_ms,
                "a_site": self.devices[l.a].site, "z_site": self.devices[l.b].site}))
            .collect()
    }

    pub fn circuit_rows(&self, rng: &mut Rng) -> Vec<Value> {
        self.links
            .iter()
            .filter(|l| !l.circuit.is_empty() && l.documented)
            .map(|l| {
                let months = 3 + rng.below(30) as i64;
                json!({"id": l.circuit, "provider": l.provider, "link": l.id, "a_site": self.devices[l.a].site, "z_site": self.devices[l.b].site, "capacity_mbps": l.capacity_mbps,
                    "monthly_cost": ((l.capacity_mbps as f64 / 1000.0) * (38.0 + l.latency_ms * 2.1)).round(), "sla": if l.capacity_mbps >= 400_000 { "99.99" } else { "99.95" },
                    "status": if l.live { "active" } else { "ceased" }, "contract_months_left": months})
            })
            .collect()
    }

    pub fn peer_rows(&self, since: &str) -> Vec<Value> {
        self.peers
            .iter()
            .map(|p| json!({"id": p.id, "device": self.devices[p.device].id, "peer_ip": p.peer_ip, "peer_asn": p.asn, "peer_name": p.name, "kind": p.kind, "state": "established",
                "prefixes_rx": p.prefixes, "prefixes_tx": if p.kind == "transit" { 412 } else { 388 }, "since": since}))
            .collect()
    }

    pub fn service_rows(&self) -> Vec<Value> {
        self.services
            .iter()
            .map(|s| json!({"id": s.id, "name": s.name, "tier": s.tier, "owner": s.owner, "kind": s.kind, "description": s.description, "slo_target": s.slo,
                "latency_slo_ms": s.latency_slo_ms, "revenue_per_min": s.revenue_per_min, "customers": self.customers.iter().filter(|c| c.services.contains(&s.id)).count(),
                "devices": s.devices.iter().map(|&d| self.devices[d].id.clone()).collect::<Vec<_>>(), "device_nums": s.devices.iter().map(|&d| self.devices[d].num).collect::<Vec<_>>(), "sites": s.sites}))
            .collect()
    }

    pub fn customer_rows(&self) -> Vec<Value> {
        self.customers
            .iter()
            .map(|c| json!({"id": c.id, "num": c.num, "name": c.name, "tier": c.tier, "site": c.site, "device": self.devices[c.device].id, "device_num": self.devices[c.device].num,
                "services": c.services, "mrr": c.mrr, "prefix": c.prefix, "port": c.port, "vlan": c.vlan, "circuit": c.circuit, "contact": format!("noc@{}.example", c.name.to_lowercase().replace([' ', 'è'], ""))}))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_is_the_same_every_time() {
        let (a, b) = (World::build(), World::build());
        assert_eq!(a.devices.len(), 64);
        assert_eq!(a.devices.iter().map(|d| d.serial.clone()).collect::<Vec<_>>(), b.devices.iter().map(|d| d.serial.clone()).collect::<Vec<_>>());
        assert_eq!(a.links.len(), b.links.len());
        assert!(a.links.len() > 120, "{} links", a.links.len());
        // Every backbone link has its other plane.
        for l in a.links.iter().filter(|l| l.kind == "backbone" && l.live) {
            let s = l.sibling.expect("a sibling");
            assert_eq!(a.links[s].sibling, a.link_by_id(&l.id));
        }
        // Ids are unique.
        let mut ids: Vec<&String> = a.links.iter().map(|l| &l.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), a.links.len());
        assert_eq!(a.customers.len(), 48);
        assert!(a.services.iter().all(|s| !s.devices.is_empty()));
    }

    #[test]
    fn the_inventory_differs_from_the_cabling() {
        let w = World::build();
        assert_eq!(w.links.iter().filter(|l| !l.documented).count(), 2);
        assert_eq!(w.links.iter().filter(|l| !l.live).count(), 2);
    }
}
