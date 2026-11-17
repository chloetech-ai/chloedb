//! Fleet checks of the disks under the copies, as each server measures
//! its own ([`crate::disks`]): a disk that reads far slower than its
//! peers' (gh, 11-17: one machine's local disk read at 12 ms a block and
//! 4 MB/s against 0.2 ms and 600 MB/s, and every page that read its shard
//! took seconds), and a node's disk near the line where Kubernetes evicts
//! every pod on the node (gh, 11-16: a build filled a node's disk and the
//! site went down with its pods).
//!
//! Each problem carries `actions`: what the command center can do about
//! it, each a call it makes after the person confirms (`method`, `path`,
//! `body`, with its `cost` and `target` said first), a drawer it opens
//! (`drawer`), or advice it cannot carry out itself (`advice`: a remedy
//! that costs money, with what it costs). Nothing is done without a
//! person's click.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{json, Value as Json};

use super::{down, fills_in, pct, short, Memory, Problem, DISK_MEMORY_MS, DISK_SPAN_MS};
use crate::coord::plan::bytes;

/// Readings of each copy's disk remembered.
const READINGS: usize = 5;
/// Readings a copy's disk must have read slowly in before it is told: one
/// slow reading may be a moment's.
const SLOW_READINGS: usize = 2;
/// The room above the eviction line, as a share of the disk, under which
/// a node's disk is a warning, and a critical problem.
const EVICT_WARN: f64 = 0.05;
const EVICT_CRIT: f64 = 0.02;
/// Hours to the line, at the last hour's rate, under which it is a
/// warning, and a critical problem.
const EVICT_WARN_H: f64 = 24.0;
const EVICT_CRIT_H: f64 = 1.0;
/// US dollars a GiB-month of a DigitalOcean volume, which a remedy's cost
/// is said in ([`crate::digitalocean::VOLUME_GB_MONTH`]).
const VOLUME_GB_MONTH: f64 = crate::digitalocean::VOLUME_GB_MONTH;

/// Milliseconds in words: "0.2 ms", "1.5 ms", "12 ms".
pub(super) fn ms(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1} s", v / 1000.0).replace(".0 s", " s")
    } else if v >= 10.0 {
        format!("{v:.0} ms")
    } else if v >= 1.0 {
        format!("{v:.1} ms").replace(".0 ms", " ms")
    } else {
        format!("{v:.2} ms").replace("0 ms", " ms").replace(". ms", " ms")
    }
}

fn times(x: f64) -> String {
    if x >= 10.0 {
        format!("{x:.0}×")
    } else {
        format!("{x:.1}×").replace(".0×", "×")
    }
}

/// The lower median of the last three of `v`: two slow readings of three
/// make a disk slow, one does not.
fn typical(v: &VecDeque<(u64, f64, Option<f64>)>) -> (f64, Option<f64>) {
    let last: Vec<&(u64, f64, Option<f64>)> = v.iter().rev().take(3).collect();
    let mut a: Vec<f64> = last.iter().map(|r| r.1).collect();
    a.sort_by(f64::total_cmp);
    let mut b: Vec<f64> = last.iter().filter_map(|r| r.2).collect();
    b.sort_by(f64::total_cmp);
    (a[(a.len() - 1) / 2], (!b.is_empty()).then(|| b[b.len() / 2]))
}

/// The volume a copy of `data` bytes would take, in GiB (a quarter more,
/// to tens), and its month at DigitalOcean's price.
fn volume_for(data: f64) -> (u64, f64) {
    let gib = ((data * 1.25 / (1u64 << 30) as f64 / 10.0).ceil() as u64 * 10).max(10);
    (gib, gib as f64 * VOLUME_GB_MONTH)
}

/// The copy's database and log, in bytes.
fn data_bytes(r: &Json) -> f64 {
    let l = &r["detail"]["load"];
    l["file_bytes"].as_f64().unwrap_or(0.0) + l["wal_bytes"].as_f64().unwrap_or(0.0)
}

fn node_of(r: &Json) -> Option<String> {
    r["detail"]["place"]["node"].as_str().filter(|n| !n.is_empty()).map(String::from)
}

fn is_primary(r: &Json) -> bool {
    r["detail"]["health"]["replication"]["role"] == "primary" || r["role"] == "primary"
}

/// A copy's searches' p99 over its last minute.
fn p99_of(r: &Json) -> Option<f64> {
    r["detail"]["minute"]["p99_ms"].as_f64().or(r["detail"]["load"]["p99_ms"].as_f64())
}

/// Disks that read far slower than their peers' ([`crate::disks::outliers`]
/// over each copy's [`typical`] reading). Answers the copies told, as
/// (shard, name), for what else is judged by them.
pub(super) fn speed(m: &mut Memory, copies: &[(usize, Json)], shards: &[Json], alone: bool, checks: &mut Vec<Json>, problems: &mut Vec<Problem>) -> Vec<(usize, String)> {
    let mut urls = HashSet::new();
    for (_, r) in copies.iter().filter(|(_, r)| !down(r)) {
        let l = &r["detail"]["load"];
        let url = r["url"].as_str().unwrap_or_default().to_string();
        urls.insert(url.clone());
        let (Some(read), Some(at)) = (l["disk_read_ms"].as_f64(), l["disk_probe_at_ms"].as_f64()) else { continue };
        // A reading taken while a scrub read the disk says less: not kept.
        if l["disk_probe_scrubbing"].as_f64() == Some(1.0) {
            continue;
        }
        let kept = m.speed.entry(url).or_default();
        if kept.back().is_none_or(|b| b.0 != at as u64) {
            kept.push_back((at as u64, read, l["disk_mb_s"].as_f64()));
        }
        while kept.len() > READINGS {
            kept.pop_front();
        }
    }
    m.speed.retain(|u, _| urls.contains(u));
    let measured: Vec<(&usize, &Json, (f64, Option<f64>), usize)> = copies
        .iter()
        .filter(|(_, r)| !down(r))
        .filter_map(|(k, r)| m.speed.get(r["url"].as_str().unwrap_or_default()).filter(|v| !v.is_empty()).map(|v| (k, r, typical(v), v.len())))
        .collect();
    let disks: Vec<(f64, Option<f64>)> = measured.iter().map(|x| x.2).collect();
    let slow: Vec<crate::disks::Outlier> = crate::disks::outliers(&disks).into_iter().filter(|o| measured[o.i].3 >= SLOW_READINGS).collect();
    let all_ms: Vec<f64> = disks.iter().map(|d| d.0).collect();
    let median = {
        let mut v = all_ms.clone();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied()
    };
    let slow_urls: HashSet<&str> = slow.iter().map(|o| measured[o.i].1["url"].as_str().unwrap_or_default()).collect();
    checks.push(json!({
        "id": "disk_speed", "status": if slow.is_empty() { "ok" } else { "warning" }, "measured": measured.len(),
        "median_read_ms": median, "where": slow.iter().map(|o| short(measured[o.i].1)).collect::<Vec<_>>(),
    }));
    let mut told = Vec::new();
    for o in &slow {
        let (k, r, (read, mb), _) = measured[o.i];
        let name = short(r);
        let url = r["url"].as_str().unwrap_or_default();
        let s = shards[*k]["shard"].as_u64().unwrap_or(*k as u64) as usize;
        told.push((s, name.clone()));
        let l = &r["detail"]["load"];
        let node = node_of(r);
        let mut where_ = if alone { vec![name.clone()] } else { vec![format!("shard {s}"), name.clone()] };
        where_.extend(node.iter().map(|n| format!("node {n}")));
        let (title, headline) = match o.peers_ms {
            Some(p) if read >= p * crate::disks::SLOW_TIMES => {
                let x = times(read / p.max(0.001));
                let row = match (o.slow_mb_s, mb, o.peers_mb_s) {
                    (true, Some(m), Some(pm)) => format!(", and {m:.0} MB/s in a row against {pm:.0}"),
                    _ => String::new(),
                };
                (format!("{name}'s disk reads {x} slower than its peers': {} a read against {}{row}", ms(read), ms(p)), format!("{name}'s disk reads {x} slower than its peers'"))
            }
            _ => match (o.slow_mb_s, mb, o.peers_mb_s) {
                (true, Some(m), Some(pm)) => (format!("{name}'s disk reads {m:.0} MB/s in a row against {pm:.0} on its peers' disks"), format!("{name}'s disk reads slowly")),
                _ => (format!("{name}'s disk reads slowly: {} a 4 KiB read", ms(read)), format!("{name}'s disk reads slowly")),
            },
        };
        // The shard's other copies, and one that could take its writes.
        let rs = shards[*k]["replicas"].as_array().cloned().unwrap_or_default();
        let others: Vec<&Json> = rs.iter().filter(|x| x["url"] != r["url"]).collect();
        let primary = is_primary(r);
        let in_sync: Vec<&str> = r["detail"]["health"]["replication"]["replicas"].as_array().into_iter().flatten().filter(|f| f["in_sync"] == true && f["copying"] != true).filter_map(|f| f["url"].as_str()).collect();
        let fast_twin = others.iter().find(|x| !down(x) && !slow_urls.contains(x["url"].as_str().unwrap_or_default()) && in_sync.contains(&x["url"].as_str().unwrap_or_default())).copied();
        let mut impact = if others.is_empty() {
            format!("Every search that reads shard {s} waits on it: what the page cache does not hold reads at {} a block", ms(read))
        } else {
            format!("Searches that read shard {s} from {name} wait on its disk{}; the coordinator asks a slow copy less, not never", if primary { ", and so does every write to the shard" } else { "" })
        };
        let theirs: Vec<f64> = others.iter().filter(|x| !down(x)).filter_map(|x| p99_of(x)).collect();
        if let Some(p) = p99_of(r) {
            if !theirs.is_empty() {
                let mut t = theirs.clone();
                t.sort_by(f64::total_cmp);
                impact.push_str(&format!(" (its searches' p99 over the last minute: {} against {} on the shard's other copies)", ms(p), ms(t[t.len() / 2])));
            } else {
                impact.push_str(&format!(" (its searches' p99 over the last minute: {})", ms(p)));
            }
        }
        let mut why = Vec::new();
        match l["disk_busy"].as_f64() {
            Some(b) if b < 0.3 => why.push(format!("The disk was busy {} of the time since the last probe: it is slow without being loaded, so the fault is the disk's, or the machine's under it (a degraded cloud volume or host), not the queries'", pct(b))),
            Some(b) if b >= 0.8 => why.push(format!("The disk was busy {} of the time since the last probe: what reads it (queries, merges, a scrub, another pod on the node) keeps it busy; Profile shows the server's share", pct(b))),
            Some(b) => why.push(format!("The disk was busy {} of the time since the last probe", pct(b))),
            None => {}
        }
        if let Some(a) = l["disk_await_ms"].as_f64() {
            why.push(format!("Every read of the device since the last probe, the queries' too, took {} on average", ms(a)));
        }
        why.push(format!("Measured by {name} itself: random 4 KiB reads of its database around the page cache, the median of the last probes"));
        let data = data_bytes(r);
        let (gib, month) = volume_for(data);
        let replace = format!(
            "Add a copy of shard {s} on another node or on a volume (Cluster: shard {s}, Add a copy), wait until it is in sync, then remove {name}. Costs money: a node at its size's price, or a volume of about {gib} GiB for {name}'s {} (about ${month:.0} a month at DigitalOcean's ${VOLUME_GB_MONTH:.2} a GiB-month). The command center buys neither",
            bytes(data)
        );
        let next = match fast_twin.filter(|_| primary) {
            Some(t) => format!("Make {} the primary so that writes go to a fast disk, then replace {name}", short(t)),
            None if others.is_empty() => format!("Replace {name}'s disk: move shard {s} to another node or a volume (a copy there, then remove this one)"),
            None => format!("Replace {name}: add a copy of shard {s} elsewhere, then remove {name}"),
        };
        let mut actions = vec![
            json!({"id": "probe", "label": "Measure again", "method": "POST", "path": "/v1/admin/disk/probe", "body": {"copy": url}, "target": name, "cost": "Free: reads 4 MiB from the disk", "api": "POST /v1/admin/disk/probe"}),
            json!({"id": "disk", "label": "See the disk", "drawer": "disk", "copy": url, "api": format!("GET /v1/admin/disk?copy={}", crate::mcp::encode(url))}),
        ];
        if let (true, Some(t)) = (primary, fast_twin) {
            let tn = short(t);
            actions.push(json!({
                "id": "promote", "label": format!("Make {tn} the primary"), "method": "POST", "path": format!("/v1/admin/shards/{s}/promote"), "body": {"url": t["url"]},
                "target": tn, "confirm": true, "api": format!("POST /v1/admin/shards/{s}/promote"),
                "cost": format!("Free. {tn} is in sync and its disk reads as its peers' do. Writes to shard {s} pause for a moment while it takes over; reads go on. {name} stays a copy"),
            }));
        }
        actions.push(json!({"id": "replace", "label": "Replace it", "advice": true, "text": replace, "cost": format!("about ${month:.0} a month for a volume, or a node's price")}));
        problems.push(Problem {
            rank: 9,
            critical: false,
            headline,
            j: json!({"id": format!("disk-slow-{name}"), "severity": "warning", "title": title, "where": where_, "since_ms": null,
                "impact": impact, "why": why, "read_ms": read, "peers_read_ms": o.peers_ms, "mb_s": mb, "peers_mb_s": o.peers_mb_s,
                "next": {"text": next, "view": "cluster", "params": {"shard": s, "copy": name}}, "actions": actions}),
        });
    }
    told
}

/// Nodes whose disk the database shares with the kubelet (a `hostPath` or
/// an `emptyDir`, [`crate::disks::Probe::place`]), near the line under
/// which Kubernetes evicts every pod of the node. Answers the URLs of
/// the copies on such disks, which the disk check of fullness leaves to
/// this one.
pub(super) fn eviction(m: &mut Memory, copies: &[(usize, Json)], shards: &[Json], now: u64, checks: &mut Vec<Json>, problems: &mut Vec<Problem>) -> HashSet<String> {
    struct Node {
        headroom: f64,
        total: f64,
        avail: f64,
        line: f64,
        db: f64,
        copies: Vec<(String, String, usize)>,
    }
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut on = HashSet::new();
    for (k, r) in copies.iter().filter(|(_, r)| !down(r)) {
        let l = &r["detail"]["load"];
        if l["disk_on_node"].as_f64() != Some(1.0) {
            continue;
        }
        let url = r["url"].as_str().unwrap_or_default().to_string();
        on.insert(url.clone());
        let (Some(h), Some(total), Some(avail)) = (l["disk_evict_headroom_bytes"].as_f64(), l["disk_total_bytes"].as_f64(), l["disk_free_bytes"].as_f64()) else { continue };
        let name = short(r);
        let s = shards[*k]["shard"].as_u64().unwrap_or(*k as u64) as usize;
        let n = nodes.entry(node_of(r).unwrap_or_else(|| name.clone())).or_insert(Node { headroom: h, total, avail, line: l["disk_evict_free_ratio"].as_f64().unwrap_or(crate::disks::EVICT_FREE), db: 0.0, copies: Vec::new() });
        n.headroom = n.headroom.min(h);
        n.avail = n.avail.min(avail);
        n.db += data_bytes(r);
        n.copies.push((name, url, s));
    }
    for (node, n) in &nodes {
        let seen = m.evict.entry(node.clone()).or_default();
        if seen.back().is_none_or(|&(t, _)| now >= t + 10_000) {
            seen.push_back((now, n.headroom));
        }
        while seen.front().is_some_and(|&(t, _)| t + DISK_MEMORY_MS < now) {
            seen.pop_front();
        }
    }
    m.evict.retain(|k, _| nodes.contains_key(k));
    let mut names: Vec<&String> = nodes.keys().collect();
    names.sort();
    let mut told = Vec::new();
    let mut least: Option<f64> = None;
    for node in names {
        let n = &nodes[node];
        let share = if n.total > 0.0 { n.headroom / n.total } else { 0.0 };
        least = Some(least.map_or(share, |l: f64| l.min(share)));
        // Hours until the line, at the rate the room shrank over the span
        // remembered (ten minutes at least).
        let hours = m.evict.get(node).and_then(|s| {
            let (&(t0, h0), &(t1, h1)) = (s.front()?, s.back()?);
            let rate = (h0 - h1) / t1.saturating_sub(t0) as f64;
            (t1 >= t0 + DISK_SPAN_MS && rate > 0.0 && h1 > 0.0).then(|| h1 / rate / 3_600_000.0)
        });
        let critical = n.headroom <= 0.0 || share <= EVICT_CRIT || hours.is_some_and(|h| h <= EVICT_CRIT_H);
        if !critical && share >= EVICT_WARN && hours.is_none_or(|h| h > EVICT_WARN_H) {
            continue;
        }
        told.push(node.clone());
        let line = pct(n.line);
        let eta = hours.map(|h| fills_in(h / 24.0));
        let (title, headline) = if n.headroom <= 0.0 {
            (
                format!("Node {node}'s disk is past the line where Kubernetes evicts its pods: {} free ({}), the line is {line}", bytes(n.avail), pct(n.avail / n.total.max(1.0))),
                format!("Kubernetes may evict every pod on node {node} at any moment: its disk is past the line"),
            )
        } else {
            (
                format!("Node {node}'s disk is {} above the line where Kubernetes evicts its pods ({line} free){}", bytes(n.headroom), eta.as_ref().map_or(String::new(), |e| format!(", which it reaches in about {e} at the last hour's rate"))),
                match &eta {
                    Some(e) => format!("Kubernetes evicts the pods on node {node} in about {e} at this rate"),
                    None => format!("Node {node}'s disk is near the line where Kubernetes evicts its pods"),
                },
            )
        };
        let names: Vec<String> = n.copies.iter().map(|c| c.0.clone()).collect();
        let mut shard_ids: Vec<usize> = n.copies.iter().map(|c| c.2).collect();
        shard_ids.sort_unstable();
        shard_ids.dedup();
        // Shards none of whose copies is on another node: dark while the pods are out.
        let dark: Vec<String> = shard_ids
            .iter()
            .filter(|s| shards.iter().find(|x| x["shard"].as_u64() == Some(**s as u64)).and_then(|x| x["replicas"].as_array()).is_some_and(|rs| rs.iter().all(|r| n.copies.iter().any(|c| r["url"].as_str() == Some(c.1.as_str())))))
            .map(|s| format!("shard {s}"))
            .collect();
        let mut impact = format!("Once its disk has under {line} free, Kubernetes evicts every pod on {node}: {} {} and so does any other pod there (an app, a coordinator)", super::both(&names), if names.len() == 1 { "stops," } else { "stop," });
        if !dark.is_empty() {
            impact.push_str(&format!("; {} {} no copy on another node, so {} searches and writes fail until the pods are back", super::both(&dark), if dark.len() == 1 { "has" } else { "have" }, if dark.len() == 1 { "its" } else { "their" }));
        }
        let used = (n.total - n.avail).max(0.0);
        let other = (used - n.db).max(0.0);
        let why = vec![
            format!("Of its {}, {} is used: {} is the database{}, and {} is not this server's (images, logs, other pods' files, a build's output, a copy left behind)", bytes(n.total), bytes(used), bytes(n.db), if names.len() > 1 { format!(" of {}", super::both(&names)) } else { String::new() }, bytes(other)),
            format!("The kubelet evicts at {line} free of the node's disk (DigitalOcean's line; --evict-free-ratio to judge by another), and the database's folder is on that disk (a hostPath or an emptyDir), not on a volume of its own"),
        ];
        let (first_name, first_url, first_shard) = n.copies[0].clone();
        let (gib, month) = volume_for(n.db);
        let mut actions = vec![json!({"id": "disk", "label": "See what fills it", "drawer": "disk", "copy": first_url, "api": format!("GET /v1/admin/disk?copy={}", crate::mcp::encode(&first_url))})];
        actions.push(json!({
            "id": "volume", "label": "Give it a volume", "advice": true,
            "text": format!("Move the database off the node's disk: a copy of each shard on a volume of its own (the operator's volume storage class), in sync, then remove the copies on {node}'s disk. Costs money: about {gib} GiB, about ${month:.0} a month at DigitalOcean's ${VOLUME_GB_MONTH:.2} a GiB-month. The command center buys none"),
            "cost": format!("about ${month:.0} a month"),
        }));
        problems.push(Problem {
            rank: if critical { 3 } else { 8 },
            critical,
            headline,
            j: json!({"id": format!("evict-{node}"), "severity": super::severity(critical), "title": title, "where": std::iter::once(format!("node {node}")).chain(shard_ids.iter().map(|s| format!("shard {s}"))).chain(names.iter().cloned()).collect::<Vec<_>>(),
                "since_ms": null, "impact": impact, "why": why, "headroom_bytes": n.headroom, "hours_to_line": hours.map(|h| (h * 10.0).round() / 10.0),
                "next": {"text": format!("Delete what is not needed from {node}'s disk (See what fills it lists it, the database's own files apart), or move the database to a volume"), "view": "cluster", "params": {"shard": first_shard, "copy": first_name}},
                "actions": actions}),
        });
    }
    let status = if problems.iter().any(|p| p.critical && p.j["id"].as_str().is_some_and(|i| i.starts_with("evict-"))) {
        "critical"
    } else if told.is_empty() {
        "ok"
    } else {
        "warning"
    };
    checks.push(json!({"id": "evict", "status": status, "nodes": nodes.len(), "where": told, "least_headroom_ratio": least.map(|l| (l * 1000.0).round() / 1000.0)}));
    on
}
