//! A region of Google Cloud as the cloud view draws it: Compute Engine's
//! machine types a GKE node may be, priced from the Cloud Billing
//! Catalog's SKUs (a vCPU-hour and a GiB-hour of each family, on demand
//! and Spot), GKE's versions there, and the project's quotas.
//!
//! Kept: the families whose nodes take Persistent Disk (the volumes GKE's
//! `standard-rwo` class makes) and run the x86-64 image without a
//! toleration: E2, N1, N2, N2D, C2, C2D, C3, C3D and T2D, their standard,
//! high-memory and high-CPU types and E2's shared-core ones. Not kept:
//! the Hyperdisk-only families (N4, C4 and others), Arm ones (GKE taints
//! their nodes), GPUs, local-SSD variants, and the memory-optimised M
//! families.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value as Json;

use crate::aws::catalog::Machine;
use crate::telemetry::now_ms;

/// Compute Engine's service in the Cloud Billing Catalog.
pub const COMPUTE_SERVICE: &str = "6F81-5844-456A";

/// GKE's cluster management fee, US dollars a cluster-hour, zonal or
/// regional (<https://cloud.google.com/kubernetes-engine/pricing>); the
/// free tier takes $74.40 a month off one zonal cluster's per billing
/// account, which no plan counts.
pub const GKE_HOUR: f64 = 0.10;

/// An external IPv4 address in use by a VM, US dollars an hour: a standard
/// VM's, and a Spot VM's (<https://cloud.google.com/vpc/network-pricing>).
pub const IPV4_HOUR: f64 = 0.005;
pub const IPV4_SPOT_HOUR: f64 = 0.0025;

/// The pods a GKE Standard node runs at most by default.
pub const MAX_PODS: u32 = 110;

/// The hours a month is billed as.
pub const HOURS_A_MONTH: f64 = 730.0;

/// The families kept, what their processors are, and the CPU filter they
/// fall under in the view.
pub const FAMILIES: [(&str, &str, &str); 9] = [
    ("e2", "E2 (Intel or AMD, cost-optimised)", "either"),
    ("n1", "N1 (Intel, first generation)", "intel"),
    ("n2", "N2 (Intel)", "intel"),
    ("n2d", "N2D (AMD EPYC)", "amd"),
    ("c2", "C2 (Intel, compute-optimised)", "intel"),
    ("c2d", "C2D (AMD EPYC, compute-optimised)", "amd"),
    ("c3", "C3 (Intel Sapphire Rapids)", "intel"),
    ("c3d", "C3D (AMD EPYC Genoa)", "amd"),
    ("t2d", "T2D (AMD EPYC Milan)", "amd"),
];

/// One quota: its metric (`CPUS`, `N2D_CPUS`, `IN_USE_ADDRESSES`, ...), and
/// how much of it is used.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Quota {
    pub metric: String,
    pub limit: f64,
    pub usage: f64,
}

impl Quota {
    pub fn free(&self) -> f64 {
        (self.limit - self.usage).max(0.0)
    }
}

/// A region and its zones.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Region {
    pub slug: String,
    pub name: String,
    pub zones: Vec<String>,
}

/// A region's machine types, priced, GKE's versions, and the quotas.
#[derive(Clone, Debug, Serialize)]
pub struct Catalog {
    pub project: String,
    pub region: String,
    pub at_ms: u64,
    /// The region's zones, in order.
    pub zones: Vec<String>,
    pub machines: Vec<Machine>,
    /// The zones (of the first three) each machine type is offered in.
    pub machine_zones: HashMap<String, Vec<String>>,
    /// GKE's versions a new cluster may start on, newest first, and its
    /// default.
    pub versions: Vec<String>,
    pub default_version: Option<String>,
    /// The region's quotas and the project's (`CPUS_ALL_REGIONS`).
    pub quotas: Vec<Quota>,
    pub project_quotas: Vec<Quota>,
    /// The service account the calls are made as, which the cluster's own
    /// command center may act as through Workload Identity.
    pub deployer: Option<String>,
    /// The project's VPC networks, and the region's subnetworks (network,
    /// name), when Compute Engine listed them: a cluster's nodes go in one.
    pub networks: Option<Vec<String>>,
    pub subnetworks: Vec<(String, String)>,
    /// Whether the prices are Google's (the Cloud Billing Catalog's).
    pub priced: bool,
    /// US dollars: a GiB-month of balanced and of SSD Persistent Disk, a
    /// cluster-hour of GKE, an hour of a node's public IPv4 address.
    pub pd_balanced_gb_month: Option<f64>,
    pub pd_ssd_gb_month: Option<f64>,
    pub gke_hour: f64,
    pub ipv4_hour: f64,
    pub ipv4_spot_hour: f64,
    /// What could not be fetched, said once each.
    pub missing: Vec<String>,
}

impl Catalog {
    pub fn quota(&self, metric: &str) -> Option<&Quota> {
        self.quotas.iter().find(|q| q.metric == metric)
    }

    pub fn project_quota(&self, metric: &str) -> Option<&Quota> {
        self.project_quotas.iter().find(|q| q.metric == metric)
    }

    /// The region's quota of vCPUs a family's VMs count against: its own
    /// (`N2D_CPUS`), else `CPUS` (E2 and N1).
    pub fn cpu_metric(&self, family: &str) -> String {
        let own = format!("{}_CPUS", family.to_ascii_uppercase());
        if self.quota(&own).is_some() {
            own
        } else {
            "CPUS".into()
        }
    }
}

/// What a machine type is for, from its name (`n2-highmem-4`), and the
/// vCPUs it is billed for (E2's shared-core types are billed a share of a
/// vCPU); `None` for those a cluster does not run on.
pub fn kind(name: &str) -> Option<(&'static str, Option<f64>)> {
    let parts: Vec<&str> = name.split('-').collect();
    if !FAMILIES.iter().any(|f| f.0 == parts[0]) {
        return None;
    }
    match parts.as_slice() {
        ["e2", "micro"] => Some(("shared", Some(0.25))),
        ["e2", "small"] => Some(("shared", Some(0.5))),
        ["e2", "medium"] => Some(("shared", Some(1.0))),
        [_, class, n] if n.chars().all(|c| c.is_ascii_digit()) => Some((
            match *class {
                "standard" => "general",
                "highmem" => "memory",
                "highcpu" => "compute",
                _ => return None,
            },
            None,
        )),
        _ => None,
    }
}

/// The CPU filter of a family: `intel`, `amd` or `either`.
pub fn cpu(family: &str) -> &'static str {
    FAMILIES.iter().find(|f| f.0 == family).map_or("either", |f| f.2)
}

/// Compute Engine's prices, by region: a vCPU-hour and a GiB-hour of each
/// family, on demand and Spot, and a GiB-month of Persistent Disk.
#[derive(Clone, Debug, Default)]
pub struct Prices {
    pub at_ms: u64,
    pub regions: HashMap<String, RegionPrices>,
    /// A public IPv4 address an hour, on a standard VM and on a Spot one
    /// (SKUs of the region `global`).
    pub ipv4: Option<f64>,
    pub ipv4_spot: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionPrices {
    pub core: HashMap<String, f64>,
    pub ram: HashMap<String, f64>,
    pub spot_core: HashMap<String, f64>,
    pub spot_ram: HashMap<String, f64>,
    pub pd_balanced: Option<f64>,
    pub pd_ssd: Option<f64>,
}

/// What a SKU's description says it prices: a family's core or RAM, on
/// demand or Spot. `N2D AMD Instance Core running in Americas`,
/// `Spot Preemptible E2 Instance Ram running in Iowa`, `N1 Predefined
/// Instance Core running in Americas`, `Compute optimized Core running in
/// Americas` (C2).
pub fn instance_part(description: &str) -> Option<(String, bool, bool)> {
    let (spot, d) = match description.strip_prefix("Spot Preemptible ").or_else(|| description.strip_prefix("Preemptible ")) {
        Some(rest) => (true, rest),
        None => (false, description),
    };
    if ["Custom", "Sole Tenancy", "Extended", "Commitment", "Upgrade", "Premium", "Reserved"].iter().any(|x| d.contains(x)) {
        return None;
    }
    let (head, core) = if let Some((h, _)) = d.split_once(" Instance Core running in ") {
        (h, true)
    } else if let Some((h, _)) = d.split_once(" Instance Ram running in ") {
        (h, false)
    } else if d.starts_with("Compute optimized Core running in ") {
        ("C2", true)
    } else if d.starts_with("Compute optimized Ram running in ") {
        ("C2", false)
    } else {
        return None;
    };
    let family = head.split_whitespace().next()?.to_ascii_lowercase();
    FAMILIES.iter().any(|f| f.0 == family).then_some((family, core, spot))
}

/// The names of the networks of a `global/networks` answer.
pub fn networks(j: &Json) -> Vec<String> {
    j["items"].as_array().into_iter().flatten().filter_map(|n| n["name"].as_str().map(String::from)).collect()
}

/// The (network, subnetwork) names of a region's `subnetworks` answer.
pub fn subnetworks(j: &Json) -> Vec<(String, String)> {
    j["items"].as_array().into_iter().flatten().filter_map(|s| Some((s["network"].as_str()?.rsplit('/').next()?.to_string(), s["name"].as_str()?.to_string()))).collect()
}

/// Whether a SKU prices what a plan does: a family's cores or memory, a
/// balanced or SSD Persistent Disk, or a public address.
pub fn wanted(s: &Json) -> bool {
    let d = s["description"].as_str().unwrap_or_default();
    instance_part(d).is_some() || d.starts_with("Balanced PD Capacity") || d.starts_with("SSD backed PD Capacity") || d.starts_with("External IP Charge on a ")
}

/// A SKU's price an hour (or a month), US dollars: its last tier's (past
/// any free usage).
fn sku_price(s: &Json) -> Option<f64> {
    let rates = s["pricingInfo"][0]["pricingExpression"]["tieredRates"].as_array()?;
    let r = rates.iter().max_by(|a, b| a["startUsageAmount"].as_f64().unwrap_or(0.0).total_cmp(&b["startUsageAmount"].as_f64().unwrap_or(0.0)))?;
    let p = &r["unitPrice"];
    let units = p["units"].as_str().and_then(|u| u.parse::<f64>().ok()).or_else(|| p["units"].as_f64()).unwrap_or(0.0);
    let nanos = p["nanos"].as_f64().unwrap_or(0.0);
    Some(units + nanos / 1e9)
}

impl Prices {
    /// From the Cloud Billing Catalog's SKUs of Compute Engine.
    pub fn from_skus(skus: &[Json]) -> Prices {
        let mut regions: HashMap<String, RegionPrices> = HashMap::new();
        let (mut ipv4, mut ipv4_spot) = (None, None);
        for s in skus {
            let d = s["description"].as_str().unwrap_or_default();
            let usage = s["category"]["usageType"].as_str().unwrap_or_default();
            let Some(mut price) = sku_price(s) else { continue };
            // A few families' memory is priced by the GB (10^9 bytes), not the GiB.
            if s["pricingInfo"][0]["pricingExpression"]["usageUnit"].as_str() == Some("GBy.h") {
                price *= 1.073_741_824;
            }
            if usage == "OnDemand" && d == "External IP Charge on a Standard VM" {
                ipv4 = Some(price);
            } else if usage == "OnDemand" && d == "External IP Charge on a Spot Preemptible VM" {
                ipv4_spot = Some(price);
            }
            let here: Vec<String> = s["serviceRegions"].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect()).unwrap_or_default();
            for r in here {
                let rp = regions.entry(r).or_default();
                if let Some((family, core, spot)) = instance_part(d) {
                    if (spot && usage != "Preemptible") || (!spot && usage != "OnDemand") {
                        continue;
                    }
                    let map = match (core, spot) {
                        (true, false) => &mut rp.core,
                        (false, false) => &mut rp.ram,
                        (true, true) => &mut rp.spot_core,
                        (false, true) => &mut rp.spot_ram,
                    };
                    map.insert(family, price);
                } else if usage == "OnDemand" && d.starts_with("Balanced PD Capacity") {
                    rp.pd_balanced = Some(price);
                } else if usage == "OnDemand" && d.starts_with("SSD backed PD Capacity") {
                    rp.pd_ssd = Some(price);
                }
            }
        }
        Prices { at_ms: now_ms(), regions, ipv4, ipv4_spot }
    }
}

/// The region's zones, in order, from its `zones` (URLs).
pub fn zones_of(region: &Json) -> Vec<String> {
    let mut z: Vec<String> = region["zones"].as_array().map(|a| a.iter().filter_map(|u| u.as_str()?.rsplit('/').next().map(String::from)).collect()).unwrap_or_default();
    z.sort();
    z
}

/// The project's regions that are up: `GET compute/v1/projects/P/regions`.
pub fn regions(j: &Json) -> Vec<Region> {
    let mut out: Vec<Region> = j["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["status"].as_str().is_none_or(|s| s == "UP"))
        .filter_map(|r| {
            let slug = r["name"].as_str()?.to_string();
            Some(Region { name: r["description"].as_str().unwrap_or(&slug).to_string(), zones: zones_of(r), slug })
        })
        .collect();
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

fn quotas(j: &Json) -> Vec<Quota> {
    j["quotas"].as_array().map(|a| a.iter().filter_map(|q| Some(Quota { metric: q["metric"].as_str()?.into(), limit: q["limit"].as_f64()?, usage: q["usage"].as_f64().unwrap_or(0.0) })).collect()).unwrap_or_default()
}

/// The machine types of `zones` (each zone's `machineTypes` answer) a GKE
/// node may be, priced from `prices` in `region`, and the zones each is in.
pub fn machines(region: &str, zones: &[(String, Json)], prices: Option<&Prices>) -> (Vec<Machine>, HashMap<String, Vec<String>>) {
    let rp = prices.and_then(|p| p.regions.get(region));
    let mut by_name: HashMap<String, Machine> = HashMap::new();
    let mut where_: HashMap<String, Vec<String>> = HashMap::new();
    for (zone, j) in zones {
        for t in j["items"].as_array().into_iter().flatten() {
            let Some(name) = t["name"].as_str() else { continue };
            let Some((kind, share)) = kind(name) else { continue };
            if t["deprecated"]["state"].as_str().is_some_and(|s| s != "ACTIVE") || t["architecture"].as_str().is_some_and(|a| a != "X86_64") || t["accelerators"].as_array().is_some_and(|a| !a.is_empty()) {
                continue;
            }
            let (Some(vcpu), Some(mb)) = (t["guestCpus"].as_u64(), t["memoryMb"].as_f64()) else { continue };
            where_.entry(name.to_string()).or_default().push(zone.clone());
            if by_name.contains_key(name) {
                continue;
            }
            let family = name.split('-').next().unwrap_or_default().to_string();
            let memory_gib = mb / 1024.0;
            let billed_vcpu = share.unwrap_or(vcpu as f64);
            let price = |core: Option<&HashMap<String, f64>>, ram: Option<&HashMap<String, f64>>| -> Option<f64> {
                let (c, r) = (core?.get(&family)?, ram?.get(&family)?);
                Some(c * billed_vcpu + r * memory_gib)
            };
            let processor = FAMILIES.iter().find(|f| f.0 == family).map(|f| f.1.to_string());
            by_name.insert(
                name.to_string(),
                Machine {
                    name: name.into(),
                    family: family.clone(),
                    kind,
                    arch: "x86_64".into(),
                    vcpu: vcpu as u32,
                    memory_gib,
                    burstable: kind == "shared",
                    local_gb: 0,
                    network: String::new(),
                    ebs_mbps: 0,
                    max_pods: MAX_PODS,
                    processor,
                    usd_hour: price(rp.map(|p| &p.core), rp.map(|p| &p.ram)),
                    spot_hour: price(rp.map(|p| &p.spot_core), rp.map(|p| &p.spot_ram)),
                },
            );
        }
    }
    let mut out: Vec<Machine> = by_name.into_values().collect();
    out.sort_by(|a, b| a.family.cmp(&b.family).then(a.kind.cmp(b.kind)).then(a.vcpu.cmp(&b.vcpu)).then(a.memory_gib.total_cmp(&b.memory_gib)));
    for z in where_.values_mut() {
        z.sort();
    }
    (out, where_)
}

/// The machines suggested for search: N2 and N2D high-memory types of 2
/// to 8 vCPUs, with a price.
pub fn recommended(machines: &[Machine]) -> Vec<&Machine> {
    machines.iter().filter(|m| (m.family == "n2" || m.family == "n2d") && m.kind == "memory" && (2..=8).contains(&m.vcpu) && m.usd_hour.is_some()).collect()
}

/// The release channel a plan's clusters are in: GKE takes a new
/// project's cluster only in one (no channel is for existing customers).
pub const CHANNEL: &str = "REGULAR";

/// GKE's versions a cluster in [`CHANNEL`] may start on (the channel's
/// `validVersions`, newest first) and the one a plan takes: the channel's
/// default, else the newest.
pub fn versions(server: &Json) -> (Vec<String>, Option<String>) {
    let strs = |a: &Json| a.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    let Some(ch) = server["channels"].as_array().and_then(|c| c.iter().find(|c| c["channel"] == CHANNEL)) else { return (Vec::new(), None) };
    let v = strs(&ch["validVersions"]);
    let default = ch["defaultVersion"].as_str().unwrap_or_default();
    let pick = v.iter().find(|x| x.as_str() == default).or_else(|| v.first()).cloned();
    (v, pick)
}

/// The catalogue of `region` from what Google answered.
#[allow(clippy::too_many_arguments)]
pub fn build(project: &str, region: &str, reg: &Json, proj: Option<&Json>, zones: &[(String, Json)], server: &Json, prices: Option<&Prices>, deployer: Option<String>, mut missing: Vec<String>) -> Catalog {
    let (machines, machine_zones) = machines(region, zones, prices);
    let (versions, default_version) = versions(server);
    let rp = prices.and_then(|p| p.regions.get(region));
    let priced = machines.iter().any(|m| m.usd_hour.is_some());
    if prices.is_some() && !priced {
        missing.push(format!("the prices of the machine types in {region}: the Cloud Billing Catalog listed none"));
    }
    Catalog {
        project: project.into(),
        region: region.into(),
        at_ms: now_ms(),
        zones: zones_of(reg),
        machines,
        machine_zones,
        versions,
        default_version,
        quotas: quotas(reg),
        project_quotas: proj.map(quotas).unwrap_or_default(),
        deployer,
        networks: None,
        subnetworks: Vec::new(),
        priced,
        pd_balanced_gb_month: rp.and_then(|p| p.pd_balanced),
        pd_ssd_gb_month: rp.and_then(|p| p.pd_ssd),
        gke_hour: GKE_HOUR,
        ipv4_hour: prices.and_then(|p| p.ipv4).unwrap_or(IPV4_HOUR),
        ipv4_spot_hour: prices.and_then(|p| p.ipv4_spot).unwrap_or(IPV4_SPOT_HOUR),
        missing,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const MACHINE_TYPES: &str = include_str!("../../tests/fixtures/gcp/machine-types.json");
    pub const SKUS: &str = include_str!("../../tests/fixtures/gcp/skus.json");
    pub const REGION: &str = include_str!("../../tests/fixtures/gcp/region.json");
    pub const PROJECT: &str = include_str!("../../tests/fixtures/gcp/project.json");
    pub const SERVER_CONFIG: &str = include_str!("../../tests/fixtures/gcp/server-config.json");

    /// The catalogue of us-central1 from the fixtures, its machine types
    /// offered in every zone.
    pub fn catalog() -> Catalog {
        let skus: Json = serde_json::from_str(SKUS).unwrap();
        let prices = Prices::from_skus(skus["skus"].as_array().unwrap());
        let types: Json = serde_json::from_str(MACHINE_TYPES).unwrap();
        let reg: Json = serde_json::from_str(REGION).unwrap();
        let zones: Vec<(String, Json)> = zones_of(&reg).into_iter().take(3).map(|z| (z, types.clone())).collect();
        build("chloedb", "us-central1", &reg, Some(&serde_json::from_str(PROJECT).unwrap()), &zones, &serde_json::from_str(SERVER_CONFIG).unwrap(), Some(&prices), Some("chloedb-deployer@chloedb.iam.gserviceaccount.com".into()), Vec::new())
    }

    #[test]
    fn skus_price_each_familys_core_and_memory() {
        assert_eq!(instance_part("N2D AMD Instance Core running in Americas"), Some(("n2d".into(), true, false)));
        assert_eq!(instance_part("Spot Preemptible E2 Instance Ram running in Iowa"), Some(("e2".into(), false, true)));
        assert_eq!(instance_part("N1 Predefined Instance Core running in Americas"), Some(("n1".into(), true, false)));
        assert_eq!(instance_part("Compute optimized Ram running in Americas"), Some(("c2".into(), false, false)));
        for no in ["N2 Custom Instance Core running in Americas", "N2 Sole Tenancy Instance Core running in Americas", "Commitment v1: N2 Cpu in Americas for 1 Year", "N4 Instance Core running in Americas", "A2 Instance Core running in Americas"] {
            assert_eq!(instance_part(no), None, "{no}");
        }
        let skus: Json = serde_json::from_str(SKUS).unwrap();
        let p = Prices::from_skus(skus["skus"].as_array().unwrap());
        let r = &p.regions["us-central1"];
        assert!(r.core["n2d"] > 0.0 && r.ram["n2d"] > 0.0 && r.spot_core["n2d"] < r.core["n2d"], "{r:?}");
        assert!(r.pd_balanced.is_some_and(|g| g > 0.0), "{r:?}");
        // Google's price list of 11-15-2026, us-central1 (tests/fixtures/gcp/README.md).
        assert_eq!((r.core["n2d"], r.ram["n2d"], r.core["e2"], r.ram["c2"]), (0.027502, 0.003686, 0.02181159, 0.004555));
        assert_eq!((r.pd_balanced, r.pd_ssd, p.ipv4, p.ipv4_spot), (Some(0.10), Some(0.17), Some(0.005), Some(0.0025)));
        // Custom types, commitments and premiums are not the predefined types' prices.
        assert_eq!((r.core["n2"], r.spot_core["n2d"]), (0.031611, 0.01463));
        assert!(!r.core.contains_key("n4") && !r.core.contains_key("t2a") && !r.core.contains_key("c4d") && !r.core.contains_key("dws"), "{r:?}");
    }

    #[test]
    fn machine_types_are_the_nodes_a_cluster_runs_on_priced() {
        assert_eq!(kind("n2-highmem-4"), Some(("memory", None)));
        assert_eq!(kind("e2-medium"), Some(("shared", Some(1.0))));
        for no in ["c3-standard-4-lssd", "n4-standard-4", "t2a-standard-4", "m1-megamem-96", "a2-highgpu-1g", "n1-megamem-96", "f1-micro"] {
            assert_eq!(kind(no), None, "{no}");
        }
        let cat = catalog();
        assert_eq!(cat.zones, ["us-central1-a", "us-central1-b", "us-central1-c", "us-central1-f"]);
        assert!(cat.priced);
        let names: Vec<&str> = cat.machines.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"n2d-highmem-4") && names.contains(&"e2-medium") && !names.iter().any(|n| n.starts_with("t2a-") || n.ends_with("-lssd") || n.starts_with("n4-")), "{names:?}");
        let skus: Json = serde_json::from_str(SKUS).unwrap();
        let r = Prices::from_skus(skus["skus"].as_array().unwrap()).regions["us-central1"].clone();
        let m = cat.machines.iter().find(|m| m.name == "n2d-highmem-4").unwrap();
        assert_eq!((m.kind, m.vcpu, m.memory_gib, m.max_pods, m.processor.as_deref()), ("memory", 4, 32.0, 110, Some("N2D (AMD EPYC)")));
        assert!((m.usd_hour.unwrap() - (4.0 * r.core["n2d"] + 32.0 * r.ram["n2d"])).abs() < 1e-9);
        // A shared-core type is billed for its share of a vCPU.
        let e = cat.machines.iter().find(|m| m.name == "e2-medium").unwrap();
        assert!(e.burstable && (e.usd_hour.unwrap() - (r.core["e2"] + 4.0 * r.ram["e2"])).abs() < 1e-9, "{e:?}");
        assert_eq!(cat.machine_zones["n2d-highmem-4"].len(), 3);
        let rec: Vec<&str> = recommended(&cat.machines).iter().map(|m| m.name.as_str()).collect();
        assert!(!rec.is_empty() && rec.iter().all(|n| n.starts_with("n2") && n.contains("-highmem-")), "{rec:?}");
        assert_eq!(cat.cpu_metric("n2d"), "N2D_CPUS");
        assert_eq!((cat.cpu_metric("e2").as_str(), cat.cpu_metric("n1").as_str()), ("E2_CPUS", "CPUS"));
        assert!(cat.project_quota("CPUS_ALL_REGIONS").is_some());
        // The Regular channel's default, of its own versions (not the
        // ones of no channel, which a new project may not take).
        assert_eq!(cat.default_version.as_deref(), Some("1.36.4-gke.2377000"));
        assert_eq!(cat.versions, ["1.37.1-gke.1552000", "1.36.5-gke.1982000", "1.36.4-gke.2377000"]);
        assert_eq!(versions(&serde_json::json!({"validMasterVersions": ["1.36.5-gke.2370000"]})), (Vec::new(), None), "no channel, no version");
        // Without prices, every machine is still there, unpriced.
        let unpriced = machines("us-central1", &[("us-central1-a".into(), serde_json::from_str(MACHINE_TYPES).unwrap())], None).0;
        assert!(unpriced.len() == cat.machines.len() && unpriced.iter().all(|m| m.usd_hour.is_none()));
    }
}
