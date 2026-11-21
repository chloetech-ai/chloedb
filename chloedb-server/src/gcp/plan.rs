//! The plan of a ChloeDB cluster on GKE (Google Kubernetes Engine): where
//! its copies go on the nodes, what it costs, what the project's quotas
//! allow, the request that makes the cluster, and the StrataCluster the
//! operator keeps on it.
//!
//! The node pool can grow by itself: with `max_nodes`, GKE's cluster
//! autoscaler adds nodes when copies the operator adds (its autoscale
//! rules, or more shards) find no room, and takes them away when they are
//! idle. With Workload Identity on (always, for a plan), the coordinators'
//! Kubernetes account may act as the service account the calls are made
//! as, so the cluster's own command center reaches Google Cloud without
//! anyone's laptop.
//!
//! Nothing here calls Google: it works from a region's [`Catalog`].

use serde::Deserialize;
use serde_json::{json, Value as Json};

use super::catalog::{Catalog, HOURS_A_MONTH, MAX_PODS};
use crate::aws::catalog::Machine;
use crate::cloud::{check_name, cluster_objects, cluster_yaml, fit, operator_yaml, Node, IMAGE, MAX_NODES, NAMESPACE};

/// The storage class GKE makes: balanced Persistent Disks, which grow in
/// place and are deleted with their claims.
pub const STORAGE_CLASS: &str = "standard-rwo";

/// The node pool a plan makes.
pub const POOL: &str = "strata";

/// The pods every node runs besides ChloeDB's (GKE's DaemonSets: logging,
/// metrics, kube-proxy, the PD driver's node pod, netd, konnectivity).
const NODE_PODS: u32 = 6;

/// The cluster's own pods: kube-dns (two), its autoscaler, metrics-server,
/// the event exporter, the default HTTP backend.
const SYSTEM_PODS: u32 = 6;

/// What GKE's DaemonSets request on every node, besides what the node
/// keeps ([`reserved_cpu`], [`allocatable_gib`]): estimated from GKE's
/// defaults, not yet measured on a live cluster.
const DAEMON_CPU: f64 = 0.3;
const DAEMON_GIB: f64 = 0.5;

/// The Workload Identity annotation of a Kubernetes account.
pub const WI_ANNOTATION: &str = "iam.gke.io/gcp-service-account";

/// What is asked for: `POST /v1/admin/gcp/plan`.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Plan {
    /// The cluster's name: the GKE cluster is `strata-<name>`.
    pub name: String,
    pub region: String,
    /// The zone of a zonal cluster (empty: the region's first).
    pub zone: String,
    /// The nodes' machine type, or [`crate::cloud::SMALLEST`].
    pub machine: String,
    pub nodes: u32,
    /// The nodes GKE's autoscaler may grow the pool to (0: it stays at
    /// `nodes`).
    pub max_nodes: u32,
    /// Each copy's volume, GiB.
    pub volume_gb: u32,
    /// Each node's boot disk, GiB (GKE's default is 100).
    pub boot_gb: u32,
    /// Spot nodes: cheaper, and taken back by Google at any time.
    pub spot: bool,
    /// A regional cluster: the control plane in three zones and the nodes
    /// spread over them, the same number in each. Zonal (the default): one
    /// zone, and one zonal cluster's fee a billing account is covered by
    /// GKE's free tier.
    pub regional: bool,
    /// The GKE version (empty: GKE's default in the region).
    pub version: String,
    /// The Google service account the coordinators act as through Workload
    /// Identity (empty: the one the calls are made as; `none`: none).
    pub workload_identity: String,
    /// The VPC network the nodes go in (empty: `default`), and its
    /// subnetwork in the region (empty: GKE's choice, which an
    /// auto-mode network has).
    pub network: String,
    pub subnetwork: String,
    /// The StrataCluster's spec, in part, as for EKS and DOKS.
    pub cluster: Json,
}

impl Default for Plan {
    fn default() -> Plan {
        Plan {
            name: "search".into(),
            region: "us-central1".into(),
            zone: String::new(),
            machine: "n2d-highmem-4".into(),
            nodes: 3,
            max_nodes: 0,
            volume_gb: 10,
            boot_gb: 30,
            spot: false,
            regional: false,
            version: String::new(),
            workload_identity: String::new(),
            network: String::new(),
            subnetwork: String::new(),
            cluster: json!({}),
        }
    }
}

impl Plan {
    pub fn cluster_name(&self) -> String {
        format!("strata-{}", self.name)
    }
}

/// The memory GKE leaves a node's pods, GiB: what it reserves (255 MiB
/// under 1 GiB; else 25% of the first 4 GiB, 20% of the next 4, 10% of the
/// next 8, 6% of the next 112 and 2% above 128), its eviction threshold
/// (100 MiB), less what its DaemonSets ask
/// (<https://cloud.google.com/kubernetes-engine/docs/concepts/plan-node-sizes>).
pub fn allocatable_gib(memory_gib: f64) -> f64 {
    let reserved = if memory_gib < 1.0 {
        255.0 / 1024.0
    } else {
        let mut r = 0.0;
        let mut below = 0.0;
        for (upto, share) in [(4.0, 0.25), (8.0, 0.20), (16.0, 0.10), (128.0, 0.06), (f64::INFINITY, 0.02)] {
            r += (memory_gib.min(upto) - below).max(0.0) * share;
            below = upto;
        }
        r
    };
    (memory_gib - reserved - 0.1 - DAEMON_GIB).max(0.0)
}

/// The CPU GKE reserves on a node of `vcpu` cores: 6% of the first core,
/// 1% of the second, 0.5% of the next two and 0.25% of the rest.
pub fn reserved_cpu(vcpu: f64) -> f64 {
    let mut r = 0.0;
    let mut below = 0.0;
    for (upto, share) in [(1.0, 0.06), (2.0, 0.01), (4.0, 0.005), (f64::INFINITY, 0.0025)] {
        r += (vcpu.min(upto) - below).max(0.0) * share;
        below = upto;
    }
    r
}

/// The cores of `m` its pods cannot request: GKE's reservation, its
/// DaemonSets', and for a shared-core type the share of its vCPUs it is
/// not given.
pub fn system_cpu(m: &Machine) -> f64 {
    let given = match super::catalog::kind(&m.name) {
        Some((_, Some(share))) => share,
        _ => f64::from(m.vcpu),
    };
    f64::from(m.vcpu) - given + reserved_cpu(given) + DAEMON_CPU
}

fn node_of(m: &Machine) -> Node {
    Node { vcpu: m.vcpu, alloc_gib: allocatable_gib(m.memory_gib), system_cpu: system_cpu(m) }
}

/// The zones the cluster's nodes are in.
fn zones(p: &Plan, cat: &Catalog) -> Result<Vec<String>, String> {
    if p.regional {
        let z: Vec<String> = cat.zones.iter().take(3).cloned().collect();
        if z.is_empty() {
            return Err(format!("{} has no zone", cat.region));
        }
        return Ok(z);
    }
    let z = if p.zone.is_empty() { cat.zones.first().cloned().ok_or_else(|| format!("{} has no zone", cat.region))? } else { p.zone.clone() };
    if !cat.zones.contains(&z) {
        return Err(format!("{z} is not a zone of {}: {}", cat.region, cat.zones.join(", ")));
    }
    Ok(vec![z])
}

/// The node price an hour: Spot or on demand.
fn hour(m: &Machine, spot: bool) -> Option<f64> {
    if spot {
        m.spot_hour
    } else {
        m.usd_hour
    }
}

/// The machine type `p` names, or for [`crate::cloud::SMALLEST`] the
/// cheapest that gives each copy the floor (by size when there are no
/// prices), offered in every one of `zones`.
fn machine<'a>(p: &Plan, cat: &'a Catalog, zones: &[String]) -> Result<&'a Machine, String> {
    let offered = |m: &Machine| cat.machine_zones.get(&m.name).is_some_and(|z| zones.iter().all(|x| z.contains(x)));
    if p.machine != crate::cloud::SMALLEST {
        let m = cat.machines.iter().find(|m| m.name == p.machine).ok_or_else(|| format!("{} is not a machine type a GKE node may be in {} (E2, N1, N2, N2D, C2, C2D, C3, C3D and T2D; no GPUs, Arm or Hyperdisk-only families)", p.machine, p.region))?;
        if !offered(m) {
            return Err(format!("{} is not offered in {}", m.name, zones.join(", ")));
        }
        return Ok(m);
    }
    let priced = cat.machines.iter().any(|m| hour(m, p.spot).is_some());
    let sizes: Vec<&Machine> = cat.machines.iter().filter(|m| offered(m) && (!priced || hour(m, p.spot).is_some())).collect();
    // Without prices, the fewest vCPUs, then the least memory, stand for
    // the cheapest.
    let offer: Vec<(f64, Node)> = sizes.iter().map(|m| (hour(m, p.spot).unwrap_or(f64::from(m.vcpu) * 1e3 + m.memory_gib), node_of(m))).collect();
    let i = crate::cloud::by_price_with_floor(&p.cluster, p.nodes, &offer, p.volume_gb, STORAGE_CLASS).into_iter().next();
    Ok(sizes[i.ok_or_else(|| format!("no machine type in {} gives each copy {} GiB and {} CPU on {} nodes: more nodes, or fewer copies", zones.join(", "), crate::cloud::COPY_FLOOR_GIB, crate::cloud::COPY_FLOOR_CPU, p.nodes))?])
}

/// A label value GKE takes: lower-case letters, digits, `_` and `-`, at
/// most 63.
pub fn label(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' { c } else { '-' }).take(63).collect()
}

/// The service account the coordinators act as, if any.
fn identity(p: &Plan, cat: &Catalog) -> Result<Option<String>, String> {
    match p.workload_identity.as_str() {
        "none" => Ok(None),
        "" => Ok(cat.deployer.clone()),
        sa if sa.contains('@') && sa.ends_with(".gserviceaccount.com") && sa.chars().all(|c| c.is_ascii_alphanumeric() || "@.-_".contains(c)) => Ok(Some(sa.to_string())),
        sa => Err(format!("workload_identity is a service account's address (NAME@PROJECT.iam.gserviceaccount.com), or none: not {sa:?}")),
    }
}

/// The plan: `Err` when it cannot be made.
pub fn plan(p: &Plan, cat: &Catalog) -> Result<Json, String> {
    check_name(&p.name)?;
    if p.region != cat.region {
        return Err(format!("the machine types are those of {}, not {}", cat.region, p.region));
    }
    let zones = zones(p, cat)?;
    let m = machine(p, cat, &zones)?;
    // A typo of a zero too many would bill the project for it.
    if !(1..=MAX_NODES).contains(&p.nodes) {
        return Err(format!("nodes is 1 to {MAX_NODES}"));
    }
    let z = zones.len() as u32;
    if p.regional && p.nodes % z != 0 {
        return Err(format!("a regional cluster has the same number of nodes in each of its {z} zones: nodes is {}, {} or {}", z, 2 * z, 3 * z));
    }
    if p.max_nodes != 0 && !(p.nodes..=MAX_NODES).contains(&p.max_nodes) {
        return Err(format!("max_nodes is 0 (no autoscaling), or {} to {MAX_NODES}", p.nodes));
    }
    if !(1..=65_536).contains(&p.volume_gb) {
        return Err("volume_gb is 1 to 65,536 (a Persistent Disk's size)".into());
    }
    if !(10..=1000).contains(&p.boot_gb) {
        return Err("boot_gb is 10 to 1,000".into());
    }
    let version = if p.version.is_empty() { cat.default_version.clone().or_else(|| cat.versions.first().cloned()).ok_or("GKE offers no version here")? } else { p.version.clone() };
    if !cat.versions.contains(&version) {
        return Err(format!("GKE's {} channel offers {} in {}, not {version}", super::catalog::CHANNEL.to_lowercase(), cat.versions.iter().take(6).cloned().collect::<Vec<_>>().join(", "), cat.region));
    }
    let wi = identity(p, cat)?;
    let (hold_from, hold_until) = hold(crate::telemetry::now_ms());
    let network = if p.network.is_empty() { "default" } else { p.network.as_str() };
    let name_ok = |n: &str| !n.is_empty() && n.len() <= 63 && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !name_ok(network) || (!p.subnetwork.is_empty() && !name_ok(&p.subnetwork)) {
        return Err("network and subnetwork are names of the project's (lower-case letters, digits and -)".into());
    }
    if let Some(nets) = &cat.networks {
        if !nets.iter().any(|n| n == network) {
            return Err(if nets.is_empty() {
                format!("the project {} has no VPC network for the nodes: make one first (gcloud compute networks create strata --project {} --subnet-mode auto), then plan with network strata", cat.project, cat.project)
            } else {
                format!("the project has no network {network}: it has {}", nets.join(", "))
            });
        }
    }
    if !p.subnetwork.is_empty() && !cat.subnetworks.iter().any(|(n, s)| n == network && s == &p.subnetwork) {
        return Err(format!("{network} has no subnetwork {} in {}", p.subnetwork, cat.region));
    }
    let node = node_of(m);
    let f = fit(&p.cluster, p.nodes, &node, p.volume_gb, STORAGE_CLASS)?;
    let most = p.max_nodes.max(p.nodes);

    let mut warnings = f.warnings(p.nodes);
    // The project's quotas: a family's vCPUs here, every region's vCPUs,
    // public addresses, balanced disks (which count as SSD) and VMs.
    let vcpus = f64::from(m.vcpu) * f64::from(most);
    let metric = cat.cpu_metric(&m.family);
    let mut quota = json!({"metric": metric, "needs": vcpus, "limit": null, "usage": null});
    let mut over = |what: String, q: Option<&super::catalog::Quota>, needs: f64, unit: &str, link: &str| {
        match q {
            Some(q) if needs > q.free() => warnings.push(format!("{what}: the project may use {} {unit} and uses {}, and the nodes need {needs}: ask Google for more first ({link})", q.limit, q.usage)),
            Some(_) => {}
            None => warnings.push(format!("{what}: the quota is not known")),
        }
    };
    let quotas_link = format!("https://console.cloud.google.com/iam-admin/quotas?project={}", cat.project);
    let q = cat.quota(&metric);
    if let Some(q) = q {
        quota["limit"] = json!(q.limit);
        quota["usage"] = json!(q.usage);
    }
    over(format!("{metric} in {}", cat.region), q, vcpus, "vCPUs", &quotas_link);
    let all = cat.project_quota("CPUS_ALL_REGIONS");
    quota["all_regions"] = json!(all);
    over("CPUS_ALL_REGIONS".into(), all, vcpus, "vCPUs in all regions", &quotas_link);
    over(format!("IN_USE_ADDRESSES in {}", cat.region), cat.quota("IN_USE_ADDRESSES"), f64::from(most), "public addresses (a node has one)", &quotas_link);
    let disk_gb = f64::from(f.shards * f.replicas) * f64::from(p.volume_gb) + f64::from(most) * f64::from(p.boot_gb);
    over(format!("SSD_TOTAL_GB in {}", cat.region), cat.quota("SSD_TOTAL_GB"), disk_gb, "GB of SSD and balanced disks", &quotas_link);
    if let Some(i) = cat.quota("INSTANCES") {
        if f64::from(most) > i.free() {
            warnings.push(format!("INSTANCES in {}: the project may run {} VMs and runs {}, and the nodes are {most}", cat.region, i.limit, i.usage));
        }
    }
    quota["addresses"] = json!(cat.quota("IN_USE_ADDRESSES"));
    quota["ssd_gb"] = json!(cat.quota("SSD_TOTAL_GB"));
    quota["disk_needs_gb"] = json!(disk_gb);

    let pods = f.per_node + NODE_PODS + (SYSTEM_PODS + f.coordinators + 1).div_ceil(p.nodes);
    if pods > MAX_PODS {
        warnings.push(format!("a node would run {pods} pods, and GKE lets a node run {MAX_PODS}: some copies would not start"));
    }
    if m.burstable {
        warnings.push(format!("{} is a shared-core type: it has a share of its vCPUs and bursts above it, so timed benchmarks should run on others", m.name));
    }
    if p.spot {
        warnings.push("Spot nodes may be taken back at any time, with 30 seconds' notice: a copy on one stops until GKE replaces the node".into());
    }
    if hour(m, p.spot).is_none() {
        warnings.push(format!("{} has no price here{}", m.name, if cat.priced { "" } else { ": Google's price list was not read (the Cloud Billing API is off)" }));
    }
    if p.max_nodes > p.nodes {
        warnings.push(format!("GKE's autoscaler may grow the pool to {} nodes: up to {}/h", p.max_nodes, hour(m, p.spot).map_or("an unknown price".into(), |h| format!("${:.2}", h * f64::from(p.max_nodes)))));
    }

    let cost = cost(p, m, cat, f.shards * f.replicas);
    let image = f.cluster["image"].as_str().unwrap_or(IMAGE).to_string();
    let name = p.cluster_name();
    let project = &cat.project;
    let location = if p.regional { cat.region.clone() } else { zones[0].clone() };
    let per_zone = p.nodes / z;
    let mut pool = json!({
        "name": POOL,
        "initialNodeCount": per_zone,
        "locations": zones,
        "config": {
            "machineType": m.name,
            "diskSizeGb": p.boot_gb,
            "diskType": "pd-balanced",
            "spot": p.spot,
            "oauthScopes": [super::SCOPE],
            "labels": {"strata.chloetech.ai/cluster": name},
            "resourceLabels": {"strata": "chloedb", "strata-cluster": label(&name)},
            "workloadMetadataConfig": {"mode": "GKE_METADATA"},
            "shieldedInstanceConfig": {"enableSecureBoot": true, "enableIntegrityMonitoring": true},
        },
        // A channel's nodes upgrade themselves (GKE requires it there);
        // the cluster's exclusion keeps them from doing so for HOLD_DAYS.
        "management": {"autoUpgrade": true, "autoRepair": true},
        "upgradeSettings": {"maxSurge": 1, "maxUnavailable": 0},
    });
    if p.max_nodes > p.nodes {
        pool["autoscaling"] = json!({"enabled": true, "totalMinNodeCount": p.nodes, "totalMaxNodeCount": p.max_nodes, "locationPolicy": "BALANCED"});
    }
    let request = json!({"cluster": {
        "name": name,
        "description": "A ChloeDB cluster, made by chloedb-server's cloud view",
        "initialClusterVersion": version,
        // GKE takes a new project's cluster only in a release channel. No
        // surprise restarts of the copies all the same: an exclusion of
        // minor and node upgrades (the control plane still takes its
        // patches) for HOLD_DAYS, the nodes upgraded when asked meanwhile.
        "releaseChannel": {"channel": super::catalog::CHANNEL},
        "maintenancePolicy": {"window": {"maintenanceExclusions": {HOLD_NAME: {
            "startTime": hold_from, "endTime": hold_until,
            "maintenanceExclusionOptions": {"scope": "NO_MINOR_OR_NODE_UPGRADES"},
        }}}},
        "resourceLabels": {"strata": "chloedb", "strata-cluster": label(&name)},
        "locations": zones,
        "workloadIdentityConfig": {"workloadPool": format!("{project}.svc.id.goog")},
        "network": network,
        "addonsConfig": {"gcePersistentDiskCsiDriverConfig": {"enabled": true}},
        "nodePools": [pool],
    }});
    let mut request = request;
    if !p.subnetwork.is_empty() {
        request["cluster"]["subnetwork"] = json!(p.subnetwork);
    }

    let loc_flag = format!("--project {project} --location {location}");
    let mut create = format!(
        "gcloud container clusters create {name} {loc_flag} --cluster-version {version} --release-channel regular --machine-type {} --num-nodes {per_zone} --disk-type pd-balanced --disk-size {} --workload-pool {project}.svc.id.goog --network {network} --labels strata=chloedb",
        m.name, p.boot_gb
    );
    if !p.subnetwork.is_empty() {
        create.push_str(&format!(" --subnetwork {}", p.subnetwork));
    }
    if p.regional {
        create.push_str(&format!(" --node-locations {}", zones.join(",")));
    }
    if p.max_nodes > p.nodes {
        create.push_str(&format!(" --enable-autoscaling --total-min-nodes {} --total-max-nodes {} --location-policy BALANCED", p.nodes, p.max_nodes));
    }
    if p.spot {
        create.push_str(" --spot");
    }
    // gcloud takes the upgrade hold on update only (`clusters create`
    // refuses --add-maintenance-exclusion-*); the API's create request
    // above carries it.
    let hold = format!(
        "gcloud container clusters update {name} {loc_flag} --add-maintenance-exclusion-name {HOLD_NAME} --add-maintenance-exclusion-start {hold_from} --add-maintenance-exclusion-end {hold_until} --add-maintenance-exclusion-scope no_minor_or_node_upgrades"
    );
    let commands = [
        create,
        hold,
        format!("gcloud container clusters get-credentials {name} {loc_flag}"),
        format!("kubectl apply -f {name}-operator.yaml"),
        crate::cloud::WAIT_CRD.to_string(),
        format!("kubectl apply -f {name}-cluster.yaml"),
        format!("kubectl -n {NAMESPACE} get strata {} -w", p.name),
    ];
    // The volumes are the PD driver's: deleted with their claims, before
    // the cluster (deleting it leaves them, and they bill).
    let teardown = [format!("kubectl delete -f {name}-cluster.yaml"), format!("kubectl -n {NAMESPACE} delete pvc --all --wait"), format!("gcloud container clusters delete {name} {loc_flag} --quiet")];

    // The coordinators' account acts as `wi`: the cluster's command center
    // then reaches Google Cloud as it, once the binding below is made.
    let mut cluster = f.cluster.clone();
    let account = format!("{}-coordinator", p.name);
    let identity = wi.as_ref().map(|sa| {
        cluster["coordinatorAccountAnnotations"] = json!({WI_ANNOTATION: sa});
        let member = format!("serviceAccount:{project}.svc.id.goog[{NAMESPACE}/{account}]");
        json!({
            "service_account": sa, "kubernetes_account": format!("{NAMESPACE}/{account}"), "member": member,
            "command": format!("gcloud iam service-accounts add-iam-policy-binding {sa} --project {project} --role roles/iam.workloadIdentityUser --member \"{member}\""),
            "why": format!("once, by someone who may change {sa}'s IAM policy (a project owner): the cluster's own command center then acts as {sa}, with no laptop. Without it, the cluster runs, and only reaches Google Cloud through this server"),
        })
    });
    let objects = cluster_objects(&p.name, &cluster, None);
    Ok(json!({
        "provider": "gcp",
        "stack": name,
        "cluster_name": name,
        "project": project,
        "region": cat.region,
        "location": location,
        "zones": zones,
        "regional": p.regional,
        "version": version,
        "machine": m,
        "layout": f.layout_json(pods, MAX_PODS),
        "autoscaling": if p.max_nodes > p.nodes { json!({"min": p.nodes, "max": p.max_nodes}) } else { Json::Null },
        "quota": quota,
        "cost": cost,
        "warnings": warnings,
        "manifests": {"operator": operator_yaml(&image)?, "cluster": cluster_yaml("A ChloeDB cluster on GKE, from chloedb-server's cloud view", &objects)},
        "objects": objects,
        "commands": crate::cloud::with_token_secret(&cluster, &commands),
        "teardown": teardown,
        "identity": identity,
        "cluster": cluster,
        "request": request,
    }))
}

/// What the plan costs, an hour and a month, line by line: the nodes, GKE's
/// fee, the volumes, the boot disks and the nodes' public addresses.
pub fn cost(p: &Plan, m: &Machine, cat: &Catalog, copies: u32) -> Json {
    let n = f64::from(p.nodes);
    let node = hour(m, p.spot);
    let gb = f64::from(copies) * f64::from(p.volume_gb);
    let boot = n * f64::from(p.boot_gb);
    let disk = cat.pd_balanced_gb_month;
    let ip = if p.spot { cat.ipv4_spot_hour } else { cat.ipv4_hour };
    let lines: [(String, Option<f64>); 5] = [
        (format!("{} × {}{}", p.nodes, m.name, if p.spot { " (Spot)" } else { "" }), node.map(|h| h * n)),
        (if p.regional { "GKE's fee, a regional cluster".to_string() } else { "GKE's fee, a zonal cluster".to_string() }, Some(cat.gke_hour)),
        (format!("{gb} GiB of balanced disks for the copies ({copies} × {} GiB)", p.volume_gb), disk.map(|d| gb * d / HOURS_A_MONTH)),
        (format!("{boot} GiB of boot disks ({} × {} GiB)", p.nodes, p.boot_gb), disk.map(|d| boot * d / HOURS_A_MONTH)),
        (format!("{} public IPv4 addresses", p.nodes), Some(ip * n)),
    ];
    let hour_total: f64 = lines.iter().filter_map(|l| l.1).sum();
    let unknown: Vec<&str> = lines.iter().filter(|l| l.1.is_none()).map(|l| l.0.as_str()).collect();
    json!({
        "hour": hour_total,
        "month": hour_total * HOURS_A_MONTH,
        "lines": lines.iter().map(|(w, h)| json!({"what": w, "hour": h, "month": h.map(|h| h * HOURS_A_MONTH)})).collect::<Vec<_>>(),
        "unknown": unknown,
        "per_node_hour": node.map(|h| h + ip + disk.map_or(0.0, |d| f64::from(p.boot_gb) * d / HOURS_A_MONTH)),
        "nodes": {"on_demand_hour": m.usd_hour.map(|h| h * n), "spot_hour": m.spot_hour.map(|h| h * n)},
        "not_counted": "network egress, load balancers, snapshots, and the free tier's $74.40 a month off one zonal cluster's fee a billing account",
    })
}

/// The name of the exclusion that holds a cluster's upgrades.
pub const HOLD_NAME: &str = "chloedb-hold";

/// How long it holds them: well inside a minor version's support, which
/// such an exclusion may not outlast.
pub const HOLD_DAYS: u64 = 90;

/// When a cluster made at `now_ms` holds its upgrades from and until
/// (RFC 3339, UTC).
pub fn hold(now_ms: u64) -> (String, String) {
    let rfc = |ms: u64| {
        let a = crate::aws::sig::amz_date(ms);
        format!("{}-{}-{}T{}:{}:{}Z", &a[0..4], &a[4..6], &a[6..8], &a[9..11], &a[11..13], &a[13..15])
    };
    (rfc(now_ms), rfc(now_ms + HOLD_DAYS * 86_400_000))
}

/// What Google is asked to check before a plan is made: whether its
/// machine type, zones and version are offered, the quotas fit, and (when
/// Resource Manager answers) which of the permissions it needs the
/// credentials hold.
pub fn checks(p: &Plan, cat: &Catalog, held: Option<&[String]>) -> Vec<Json> {
    let mut out = Vec::new();
    let mut check = |what: String, ok: bool| out.push(json!({"check": what, "ok": ok}));
    let zs = zones(p, cat);
    let m = zs.as_ref().ok().and_then(|z| machine(p, cat, z).ok());
    check(
        format!("GKE runs in {} and offers {} there", zs.as_ref().map_or(p.region.clone(), |z| z.join(", ")), m.map_or(p.machine.as_str(), |m| m.name.as_str())),
        cat.region == p.region && m.is_some(),
    );
    let version = if p.version.is_empty() { cat.default_version.clone().unwrap_or_default() } else { p.version.clone() };
    check(format!("GKE's {} channel offers {version}", super::catalog::CHANNEL.to_lowercase()), cat.versions.contains(&version));
    if let Some(m) = m {
        let most = f64::from(p.max_nodes.max(p.nodes));
        let vcpus = f64::from(m.vcpu) * most;
        let metric = cat.cpu_metric(&m.family);
        for (what, q, needs) in [
            (format!("{vcpus} vCPUs fit {metric} in {}", cat.region), cat.quota(&metric), vcpus),
            (format!("{vcpus} vCPUs fit CPUS_ALL_REGIONS"), cat.project_quota("CPUS_ALL_REGIONS"), vcpus),
            (format!("{most} public addresses fit IN_USE_ADDRESSES"), cat.quota("IN_USE_ADDRESSES"), most),
        ] {
            match q {
                Some(q) => check(format!("{what} ({} free of {})", q.free(), q.limit), needs <= q.free()),
                None => check(format!("{what}: the quota is not known"), false),
            }
        }
    }
    match held {
        Some(h) => {
            for perm in PERMISSIONS {
                check(format!("the credentials hold {perm}"), h.iter().any(|x| x == perm));
            }
        }
        None => check("which permissions the credentials hold: Resource Manager did not answer (enable cloudresourcemanager.googleapis.com)".into(), false),
    }
    out
}

/// What making, following and deleting a cluster needs (Kubernetes Engine
/// Admin, and Service Account User for the nodes' account), and reading
/// the region (Compute Engine).
pub const PERMISSIONS: [&str; 7] = ["container.clusters.create", "container.clusters.delete", "container.clusters.get", "container.clusters.update", "container.operations.get", "compute.machineTypes.list", "iam.serviceAccounts.actAs"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcp::catalog::tests::catalog;

    #[test]
    fn gke_leaves_its_pods_what_its_docs_say() {
        // 4 GiB: 1 GiB reserved; 32 GiB: 1 + 0.8 + 0.8 + 0.96 = 3.56.
        assert!((allocatable_gib(4.0) - (3.0 - 0.1 - 0.5)).abs() < 1e-9);
        assert!((allocatable_gib(32.0) - (32.0 - 3.56 - 0.1 - 0.5)).abs() < 1e-9);
        assert!((allocatable_gib(0.5)).abs() < 1e-9);
        // 4 cores: 60 + 10 + 10 millicores; e2-medium is given one of its two.
        assert!((reserved_cpu(4.0) - 0.08).abs() < 1e-9);
        assert!((reserved_cpu(1.0) - 0.06).abs() < 1e-9);
        let cat = catalog();
        let e2 = cat.machines.iter().find(|m| m.name == "e2-medium").unwrap();
        assert!((system_cpu(e2) - (1.0 + 0.06 + 0.3)).abs() < 1e-9);
    }

    #[test]
    fn a_plan_has_its_layout_cost_quotas_request_and_manifests() {
        let cat = catalog();
        let p = Plan { cluster: json!({"shards": 2, "replicas": 2}), nodes: 2, machine: "n2d-highmem-4".into(), ..Plan::default() };
        let j = plan(&p, &cat).unwrap();
        assert_eq!((j["cluster_name"].as_str(), j["location"].as_str(), j["version"].as_str()), (Some("strata-search"), Some("us-central1-a"), Some("1.36.4-gke.2377000")));
        assert_eq!(j["cluster"]["storage"]["storageClassName"], "standard-rwo");
        let r = &j["request"]["cluster"];
        assert_eq!(r["workloadIdentityConfig"]["workloadPool"], "chloedb.svc.id.goog");
        let pool = &r["nodePools"][0];
        assert_eq!((pool["initialNodeCount"].as_u64(), pool["config"]["machineType"].as_str(), pool["management"]["autoUpgrade"].as_bool()), (Some(2), Some("n2d-highmem-4"), Some(true)));
        assert_eq!(r["releaseChannel"]["channel"], "REGULAR");
        let x = &r["maintenancePolicy"]["window"]["maintenanceExclusions"]["chloedb-hold"];
        assert_eq!(x["maintenanceExclusionOptions"]["scope"], "NO_MINOR_OR_NODE_UPGRADES");
        assert!(x["startTime"].as_str().unwrap().ends_with('Z') && x["startTime"].as_str() < x["endTime"].as_str(), "{x}");
        assert_eq!(hold(1_763_270_400_000), ("2025-11-16T05:20:00Z".to_string(), "2026-02-14T05:20:00Z".to_string()));
        assert!(pool.get("autoscaling").is_none());
        assert_eq!(r["resourceLabels"]["strata"], "chloedb");
        // 2 × the machine, GKE's fee, 4 × 10 + 2 × 30 GiB of disk, 2 addresses.
        let m = j["machine"]["usd_hour"].as_f64().unwrap();
        let d = cat.pd_balanced_gb_month.unwrap();
        let want = 2.0 * m + 0.10 + 100.0 * d / 730.0 + 0.01;
        assert!((j["cost"]["hour"].as_f64().unwrap() - want).abs() < 1e-9, "{}", j["cost"]);
        assert_eq!(j["quota"]["metric"], "N2D_CPUS");
        assert_eq!(j["quota"]["needs"], 8.0);
        assert_eq!(j["warnings"], json!([]), "{}", j["warnings"]);
        // The coordinators act as the deployer through Workload Identity.
        assert_eq!(j["cluster"]["coordinatorAccountAnnotations"][WI_ANNOTATION], "chloedb-deployer@chloedb.iam.gserviceaccount.com");
        assert_eq!(j["identity"]["member"], "serviceAccount:chloedb.svc.id.goog[strata/search-coordinator]");
        let manifests = j["manifests"]["cluster"].as_str().unwrap();
        assert!(manifests.contains("iam.gke.io/gcp-service-account") && manifests.contains("\"kind\": \"StrataCluster\""), "{manifests}");
        assert!(j["commands"][0].as_str().unwrap().starts_with("gcloud container clusters create strata-search --project chloedb --location us-central1-a --cluster-version 1.36.4-gke.2377000 --release-channel regular --machine-type"), "{}", j["commands"][0]);
        assert!(j["commands"][1].as_str().unwrap().starts_with("gcloud container clusters update strata-search --project chloedb --location us-central1-a --add-maintenance-exclusion-name chloedb-hold"), "{}", j["commands"][1]);
        assert!(!j["commands"][0].as_str().unwrap().contains("maintenance-exclusion"), "{}", j["commands"][0]);
        assert!(j["teardown"][2].as_str().unwrap().ends_with("--quiet"));
        let none = plan(&Plan { workload_identity: "none".into(), ..p.clone() }, &cat).unwrap();
        assert!(none["identity"].is_null() && none["cluster"].get("coordinatorAccountAnnotations").is_none());

        // 12 vCPUs in all regions: 4 nodes of 4 do not fit, nor does the
        // autoscaler's 6, and the warning says so.
        let big = plan(&Plan { nodes: 4, ..p.clone() }, &cat).unwrap();
        assert!(big["warnings"].to_string().contains("CPUS_ALL_REGIONS: the project may use 12 vCPUs in all regions"), "{}", big["warnings"]);
        let auto = plan(&Plan { max_nodes: 6, ..p.clone() }, &cat).unwrap();
        assert_eq!(auto["request"]["cluster"]["nodePools"][0]["autoscaling"], json!({"enabled": true, "totalMinNodeCount": 2, "totalMaxNodeCount": 6, "locationPolicy": "BALANCED"}));
        assert!(auto["commands"][0].as_str().unwrap().contains("--enable-autoscaling --total-min-nodes 2 --total-max-nodes 6"));
        assert!(auto["warnings"].to_string().contains("CPUS_ALL_REGIONS"), "{}", auto["warnings"]);
        let c = checks(&Plan { max_nodes: 6, ..p.clone() }, &cat, Some(&PERMISSIONS.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
        assert!(c.iter().any(|c| c["ok"] == false && c["check"].as_str().unwrap().contains("CPUS_ALL_REGIONS")), "{c:?}");

        // A regional cluster: three zones, the same number of nodes in each.
        let reg = plan(&Plan { regional: true, nodes: 3, ..p.clone() }, &cat).unwrap();
        assert_eq!((reg["location"].as_str(), reg["request"]["cluster"]["nodePools"][0]["initialNodeCount"].as_u64()), (Some("us-central1"), Some(1)));
        assert_eq!(reg["zones"], json!(["us-central1-a", "us-central1-b", "us-central1-c"]));
        assert!(plan(&Plan { regional: true, nodes: 4, ..p.clone() }, &cat).unwrap_err().contains("same number of nodes in each"));
        assert!(plan(&Plan { version: "1.20.0-gke.1".into(), ..p.clone() }, &cat).unwrap_err().contains("regular channel offers"));
        // A version of no channel only (GKE refuses it to a new project).
        assert!(plan(&Plan { version: "1.36.5-gke.2370000".into(), ..p.clone() }, &cat).unwrap_err().contains("regular channel offers"));
        assert!(plan(&Plan { machine: "n4-standard-4".into(), ..p.clone() }, &cat).unwrap_err().contains("is not a machine type"));
        assert!(plan(&Plan { zone: "us-east1-b".into(), ..p.clone() }, &cat).unwrap_err().contains("is not a zone of us-central1"));
        assert!(plan(&Plan { max_nodes: 1, ..p.clone() }, &cat).unwrap_err().contains("max_nodes"));
        assert!(plan(&Plan { workload_identity: "x".into(), ..p.clone() }, &cat).is_err());
        // The nodes' network: the project's default, or one it has.
        assert_eq!(r["network"], "default");
        let mut nets = catalog();
        nets.networks = Some(Vec::new());
        assert!(plan(&p, &nets).unwrap_err().contains("gcloud compute networks create strata"));
        nets.networks = Some(vec!["strata".into()]);
        nets.subnetworks = vec![("strata".into(), "strata-us".into())];
        assert!(plan(&p, &nets).unwrap_err().contains("has no network default: it has strata"));
        let custom = plan(&Plan { network: "strata".into(), subnetwork: "strata-us".into(), ..p.clone() }, &nets).unwrap();
        assert_eq!((custom["request"]["cluster"]["network"].as_str(), custom["request"]["cluster"]["subnetwork"].as_str()), (Some("strata"), Some("strata-us")));
        assert!(custom["commands"][0].as_str().unwrap().contains("--network strata --labels strata=chloedb --subnetwork strata-us"));
        assert!(plan(&Plan { network: "strata".into(), subnetwork: "other".into(), ..p.clone() }, &nets).is_err());
        let spot = plan(&Plan { spot: true, ..p.clone() }, &cat).unwrap();
        assert_eq!(spot["request"]["cluster"]["nodePools"][0]["config"]["spot"], true);
        assert!(spot["cost"]["hour"].as_f64().unwrap() < j["cost"]["hour"].as_f64().unwrap());
    }

    #[test]
    fn the_smallest_type_is_the_cheapest_that_gives_each_copy_the_floor() {
        let cat = catalog();
        for (shards, replicas, nodes) in [(1, 2, 2), (2, 2, 2), (2, 3, 3)] {
            let p = Plan { machine: crate::cloud::SMALLEST.into(), nodes, cluster: json!({"shards": shards, "replicas": replicas}), ..Plan::default() };
            let j = plan(&p, &cat).unwrap();
            let name = j["machine"]["type"].as_str().unwrap();
            let price = j["machine"]["usd_hour"].as_f64().unwrap();
            assert!(!j["warnings"].to_string().contains("each copy"), "{name}: {}", j["warnings"]);
            for m in cat.machines.iter().filter(|m| m.usd_hour.is_some_and(|u| u < price)) {
                let cheaper = plan(&Plan { machine: m.name.clone(), ..p.clone() }, &cat);
                assert!(cheaper.is_err() || cheaper.unwrap()["warnings"].to_string().contains("each copy"), "{} is cheaper than {name} and fits", m.name);
            }
        }
        // Without prices, the smallest by size, and the plan says the price is unknown.
        let mut unpriced = catalog();
        for m in &mut unpriced.machines {
            m.usd_hour = None;
            m.spot_hour = None;
        }
        unpriced.priced = false;
        let j = plan(&Plan { machine: crate::cloud::SMALLEST.into(), cluster: json!({"shards": 1, "replicas": 2}), nodes: 2, ..Plan::default() }, &unpriced).unwrap();
        assert!(j["warnings"].to_string().contains("Cloud Billing API is off"), "{}", j["warnings"]);
        assert!(j["cost"]["unknown"].as_array().is_some_and(|u| !u.is_empty()));
    }
}
