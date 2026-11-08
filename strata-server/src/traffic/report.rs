//! A test run's report: what it measured, how the cluster did against
//! its SLO and its target (a grade and the checks behind it), the model
//! fitted to it, what it costs, and advice: shapes that hold more, cost
//! less or answer faster, each with how much (`POST
//! /v1/admin/simulator/predict` answers any other shape).

use std::collections::BTreeMap;

use serde_json::{json, Value as Json};

use super::model::{Fit, KindStats, Prediction, Shape, WriteStats};
use super::run::Settings;
use crate::ApiError;

/// Where the reference prices come from.
pub const REFERENCE_SOURCE: &str = "DigitalOcean's list prices of 1 November 2026 (nyc3, a month of 672 hours); save a token in Cloud setup for a region's live prices";

/// Sizes a copy may run on, with their prices: DigitalOcean's (the
/// response of `GET /v2/sizes` kept in tests/fixtures/digitalocean).
pub fn reference_machines() -> Vec<Json> {
    const M: &[(&str, u32, f64, f64, bool)] = &[
        ("s-2vcpu-4gb", 2, 4.0, 24.0, true),
        ("s-4vcpu-8gb", 4, 8.0, 48.0, true),
        ("m-2vcpu-16gb", 2, 16.0, 84.0, false),
        ("m-4vcpu-32gb", 4, 32.0, 168.0, false),
        ("g-8vcpu-32gb", 8, 32.0, 252.0, false),
        ("m-8vcpu-64gb", 8, 64.0, 336.0, false),
        ("c-16", 16, 32.0, 336.0, false),
        ("m-16vcpu-128gb", 16, 128.0, 672.0, false),
    ];
    M.iter()
        .map(|(name, vcpu, mem, month, shared)| json!({"name": name, "vcpu": vcpu, "memory_gib": mem, "usd_month": month, "usd_hour": month / 672.0, "shared_cpu": shared, "source": "reference"}))
        .collect()
}

/// A machine: its name, cores, memory and price a month.
#[derive(Clone, Debug)]
pub struct Machine {
    pub name: String,
    pub vcpu: f64,
    pub memory_gib: Option<f64>,
    pub usd_month: Option<f64>,
    pub shared: bool,
}

impl Machine {
    pub fn of(j: &Json) -> Result<Machine, ApiError> {
        let name = j["name"].as_str().ok_or_else(|| ApiError::bad("a machine needs a name"))?.to_string();
        let vcpu = j["vcpu"].as_f64().filter(|v| *v > 0.0).ok_or_else(|| ApiError::bad(format!("machine {name} needs its vcpu")))?;
        let month = j["usd_month"].as_f64().or_else(|| j["usd_hour"].as_f64().map(|h| h * 730.0));
        Ok(Machine { name, vcpu, memory_gib: j["memory_gib"].as_f64(), usd_month: month, shared: j["shared_cpu"].as_bool().unwrap_or(false) })
    }

    /// The cores a copy has on it: less what a Kubernetes node keeps.
    pub fn cores(&self) -> f64 {
        (self.vcpu - crate::digitalocean::plan::SYSTEM_CPU).max(0.5)
    }

    fn json(&self) -> Json {
        json!({"name": self.name, "vcpu": self.vcpu, "memory_gib": self.memory_gib, "usd_month": self.usd_month, "shared_cpu": self.shared})
    }
}

/// A machine by its name among `list`, or as given.
pub fn machine(given: Option<&Json>, list: &[Json], cores: f64) -> Machine {
    if let Some(m) = given.and_then(|g| Machine::of(g).ok()) {
        return m;
    }
    if let Some(name) = given.and_then(|g| g.as_str()) {
        if let Some(m) = list.iter().find(|m| m["name"] == name).and_then(|m| Machine::of(m).ok()) {
            return m;
        }
    }
    // The dedicated size nearest the cores the copies say they have.
    list.iter()
        .filter_map(|m| Machine::of(m).ok())
        .filter(|m| !m.shared)
        .min_by(|a, b| (a.vcpu - cores).abs().total_cmp(&(b.vcpu - cores).abs()).then(a.usd_month.unwrap_or(f64::MAX).total_cmp(&b.usd_month.unwrap_or(f64::MAX))))
        .unwrap_or(Machine { name: "this cluster's machines".into(), vcpu: cores.max(1.0), memory_gib: None, usd_month: None, shared: false })
}

/// Nodes a shape runs on: a copy each, and a coordinator each.
pub fn nodes(s: &Shape) -> usize {
    (s.shards * s.copies + s.coordinators).max(1)
}

pub fn cost(s: &Shape, m: &Machine) -> Option<f64> {
    m.usd_month.map(|p| p * nodes(s) as f64)
}

/// A shape on a machine.
pub fn on(s: Shape, m: &Machine) -> Shape {
    Shape { cores: m.cores(), coord_cores: m.cores(), ..s }
}

/// A prediction as the page shows it.
pub fn cell(fit: &Fit, s: &Shape, m: &Machine, slo_ms: f64, qps: f64) -> Json {
    let p = fit.predict(s, slo_ms, Some(qps));
    let at = p.at.as_ref();
    let mem = m.memory_gib.zip(fit_bytes(fit)).map(|(gib, per_doc)| per_doc * s.docs / s.shards.max(1) as f64 / (gib * (1u64 << 30) as f64));
    json!({
        "shards": s.shards, "copies": s.copies, "coordinators": s.coordinators, "machine": m.name, "cores": s.cores, "docs": s.docs, "nodes": nodes(s),
        "capacity_qps": finite(p.capacity_qps), "saturation_qps": finite(p.saturation_qps), "bottleneck": p.bottleneck,
        "qps": qps, "p50_ms": at.and_then(|a| a.p50_ms), "p99_ms": at.and_then(|a| a.p99_ms),
        "busy": at.map(|a| json!({"copies": a.busy.copy, "coordinator": a.busy.coordinator, "writer": a.busy.writer})),
        "idle_p99_ms": p.idle.p99_ms,
        "holds": at.and_then(|a| a.p99_ms).is_some_and(|x| x <= slo_ms),
        "cost_month": cost(s, m),
        "memory_share": mem,
    })
}

fn finite(x: f64) -> Json {
    if x.is_finite() {
        json!(x)
    } else {
        Json::Null
    }
}

/// Bytes of a document of the test table on a copy, if known.
fn fit_bytes(_fit: &Fit) -> Option<f64> {
    None
}

/// The p99 curve of a shape: its p99 at rates up to saturation.
pub fn curve(fit: &Fit, s: &Shape, points: usize) -> Vec<Json> {
    let sat = fit.saturation(s);
    if !sat.is_finite() || sat <= 0.0 {
        return Vec::new();
    }
    (1..=points)
        .map(|i| {
            let q = sat * 0.98 * i as f64 / points as f64;
            json!([q, fit.at(s, q).p99_ms])
        })
        .collect()
}

pub struct Inputs<'a> {
    pub set: &'a Settings,
    pub fit: &'a Fit,
    pub stats: &'a [KindStats],
    pub write: &'a WriteStats,
    pub steps: &'a [Json],
    /// Measured: capacity, whether only a bound, the last rate held, the first that failed.
    pub capacity: (f64, bool, Option<f64>, Option<f64>),
    pub kappa: f64,
    pub rows_now: u64,
    pub servers: &'a [Json],
    pub data: Json,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

fn rate(x: f64) -> String {
    if x >= 100.0 {
        let n = x.round() as u64;
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        out
    } else if x >= 10.0 {
        format!("{x:.0}")
    } else {
        format!("{x:.1}")
    }
}

fn ms(x: f64) -> String {
    if x >= 100.0 {
        format!("{x:.0} ms")
    } else if x >= 10.0 {
        format!("{x:.1} ms")
    } else {
        format!("{x:.2} ms")
    }
}

fn money(x: f64) -> String {
    format!("${}", rate(x.round()))
}

fn check(id: &str, state: &str, title: &str, text: String) -> Json {
    json!({"id": id, "state": state, "title": title, "text": text})
}

/// The report of a run.
pub fn build(i: Inputs) -> Json {
    let set = i.set;
    let fit = i.fit;
    let slo = set.slo_ms;
    let tested = fit.tested_shape();
    let (cap, bound, held, failed) = i.capacity;
    let machines = reference_machines();
    let copy_cores_reported = i.steps.last().and_then(|s| s["busy"]["servers"].as_array()).and_then(|a| a.iter().filter_map(|x| x["cores"].as_f64()).reduce(f64::max)).unwrap_or(fit.tested.copy_cores);
    let m = machine(set.config.machine.as_ref(), &machines, copy_cores_reported);
    let docs_est = set.config.estimate_docs.unwrap_or(if i.rows_now > fit.tested.docs { i.rows_now as f64 } else { fit.tested.docs as f64 });
    let base = on(Shape { docs: docs_est, ..tested }, &m);
    let base_p = fit.predict(&base, slo, None);
    let target = set.config.target_qps;
    let at_rate = target.unwrap_or_else(|| if cap > 0.0 { cap * 0.5 } else { base_p.capacity_qps * 0.5 }).max(1e-3);
    // The tested cluster as the model has it, at the test's rows; and the
    // shape the report speaks of: that, or, when its servers shared a
    // machine (which the model does not price), the same shape with a
    // machine each.
    let tested_p = fit.predict(&tested, slo, target.or(Some(at_rate)));
    let shared = i.data["shared_host"] == true;
    let (main_shape, main_p) = if shared { (base, fit.predict(&base, slo, Some(at_rate))) } else { (tested, tested_p.clone()) };

    // ------------------------------------------------ the checks
    let mut checks = Vec::new();
    let steady = set.config.shape == "steady";
    let shown_cap = if steady { None } else { Some(cap) };
    if let Some(cap) = shown_cap {
        let said = if bound { format!("at least {} requests a second (no step missed the SLO)", rate(cap)) } else { format!("{} requests a second", rate(cap)) };
        match target {
            Some(t) => {
                let head = cap / t - 1.0;
                let (st, how) = if head >= 0.3 {
                    ("ok", format!("{} over your target of {}/s", pct(head), rate(t)))
                } else if head >= 0.0 {
                    ("warn", format!("only {} over your target of {}/s: a burst or a slow copy would push it over", pct(head), rate(t)))
                } else {
                    ("bad", format!("{} short of your target of {}/s", pct(-head), rate(t)))
                };
                checks.push(check("capacity", st, "Capacity", format!("It holds {said} within a p99 of {}: {how}.", ms(slo))));
            }
            None => checks.push(check("capacity", "info", "Capacity", format!("It holds {said} within a p99 of {}. Set a target rate to grade it.", ms(slo)))),
        }
    } else if let Some(last) = i.steps.last() {
        let held_all = i.steps.iter().all(|s| s["holds"] == true);
        let r = set.config.rate.unwrap_or(0.0);
        checks.push(check("capacity", if held_all { "ok" } else { "bad" }, "Holding the rate", if held_all { format!("Every window held {}/s within a p99 of {}.", rate(r), ms(slo)) } else { format!("{} of {} windows at {}/s missed the p99 of {} (the last: {}).", i.steps.iter().filter(|s| s["holds"] != true).count(), i.steps.len(), rate(r), ms(slo), ms(last["p99_ms"].as_f64().unwrap_or(0.0))) }));
    }
    // One machine's cores: the limit of a test whose servers share it.
    if i.data["shared_host"] == true {
        let cores = i.data["host_cores"].as_f64().unwrap_or(0.0);
        let text = match i.data["host_at_limit"].as_object() {
            Some(l) => {
                let used = l["servers_cores"].as_f64().unwrap_or(0.0);
                let gen = l["load_generator_cores"].as_f64().unwrap_or(0.0);
                format!(
                    "This cluster's servers share a machine of {} cores: at {}/s, the busiest step that held, they used {:.1} of them{}. So the limit measured is that machine's; the shapes predicted give each copy a machine of its own, from each request's CPU as measured.",
                    cores,
                    rate(l["rate"].as_f64().unwrap_or(0.0)),
                    used,
                    if gen >= 0.05 { format!(" and the test's own load generator {gen:.1}") } else { String::new() }
                )
            }
            None => format!("This cluster's servers share a machine of {cores} cores, so the limit measured is that machine's; the shapes predicted give each copy a machine of its own."),
        };
        checks.push(check("host", "info", "One machine", text));
    }
    // Latency at no load: the kinds alone.
    let idle_p99 = fit.kinds.iter().filter(|k| k.share > 0.0).map(|k| (k.kind.clone(), k.lat_p99)).max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((kind, p)) = &idle_p99 {
        let (st, text) = if *p >= slo {
            ("bad", format!("Even alone, one at a time, {kind} takes {} at its p99, over the SLO's {}: no number of copies brings it under; fewer rows a shard (more shards) or a cheaper query does.", ms(*p), ms(slo)))
        } else if *p >= slo * 0.5 {
            ("warn", format!("Alone, {kind} already takes {} at its p99, half the SLO or more: queues at load leave little room.", ms(*p)))
        } else {
            ("ok", format!("Alone, the slowest kind ({kind}) takes {} at its p99, against an SLO of {}.", ms(*p), ms(slo)))
        };
        checks.push(check("latency", st, "Latency at no load", text));
    }
    // What saturates first.
    let busiest = i.steps.iter().filter(|s| s["holds"] == true).max_by(|a, b| a["rate"].as_f64().unwrap_or(0.0).total_cmp(&b["rate"].as_f64().unwrap_or(0.0))).or(i.steps.last());
    let first = main_p.bottleneck.clone();
    let first_text = match first.as_str() {
        "coordinator" => "the coordinator: it merges every shard's answers, and more copies do not help it; another coordinator (or a bigger one) does".to_string(),
        "writer" => "the shards' writers: each shard applies its writes on one writer, on every copy; more shards spread the writes, more copies do not".to_string(),
        _ => format!("the copies' cores (reads spread over the {} copies of each shard: more copies, or bigger machines, hold more)", tested.copies),
    };
    let on_what = if shared { format!(" with one {} each", m.name) } else { String::new() };
    checks.push(check("bottleneck", if first == "copies" { "info" } else { "warn" }, "What fills first", format!("By the model{on_what}, {first_text}. It saturates at about {}/s.", rate(main_p.saturation_qps))));
    // Balance at the busiest step that held: between a shard's copies
    // (reads spread over them), then between the shards (their rows).
    if let Some(b) = busiest {
        let used: Vec<(String, usize, f64)> = b["busy"]["servers"].as_array().into_iter().flatten().filter_map(|s| Some((s["name"].as_str()?.to_string(), s["shard"].as_u64()? as usize, s["cores_used"].as_f64()?))).collect();
        let at = rate(b["rate"].as_f64().unwrap_or(0.0));
        let mut shards: BTreeMap<usize, Vec<(String, f64)>> = BTreeMap::new();
        for (name, shard, u) in &used {
            shards.entry(*shard).or_default().push((name.clone(), *u));
        }
        // A copy that did far less than its shard's busiest.
        let idle_copy = shards.iter().filter(|(_, c)| c.len() > 1).find_map(|(shard, c)| {
            let top = c.iter().map(|x| x.1).fold(0.0, f64::max);
            let low = c.iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
            (top > 0.05 && low.1 < 0.5 * top).then(|| (*shard, low.0.clone(), low.1 / top))
        });
        let totals: Vec<(usize, f64)> = shards.iter().map(|(s, c)| (*s, c.iter().map(|x| x.1).sum::<f64>())).collect();
        let mean = totals.iter().map(|t| t.1).sum::<f64>() / totals.len().max(1) as f64;
        let hot = totals.iter().max_by(|a, b| a.1.total_cmp(&b.1)).copied();
        if let Some((shard, name, share)) = idle_copy {
            checks.push(check("balance", "warn", "Balance", format!("{name} did {} of the work of shard {shard}'s busiest copy at {at}/s: the coordinator sent it fewer reads (it was behind, or answered slower), so its shard's other copies carried them, and a copy more of that shard adds less than the model says.", pct(share))));
        } else if let (Some((shard, top)), true) = (hot, totals.len() > 1 && mean > 0.05) {
            if top > 1.3 * mean {
                checks.push(check("balance", "warn", "Balance", format!("Shard {shard} did {} more work than the shards' mean at {at}/s: it holds more of the rows, or more popular ones. Even out its slots (Cluster, Rebalance), or give it a copy more.", pct(top / mean - 1.0))));
            } else {
                checks.push(check("balance", "ok", "Balance", format!("The shards and their copies shared the work evenly at {at}/s (the busiest shard {} over the mean).", pct(top / mean - 1.0))));
            }
        }
    }
    // Errors over the steps that held, and those that did not.
    let errs: u64 = i.steps.iter().map(|s| s["read_errors"].as_u64().unwrap_or(0)).sum();
    let reads: u64 = i.steps.iter().map(|s| s["reads"].as_u64().unwrap_or(0)).sum();
    let held_errs: u64 = i.steps.iter().filter(|s| s["holds"] == true).map(|s| s["read_errors"].as_u64().unwrap_or(0)).sum();
    let top_error = i.steps.iter().flat_map(|s| s["errors"].as_array().cloned().unwrap_or_default()).max_by_key(|e| e["n"].as_u64().unwrap_or(0));
    checks.push(if errs == 0 {
        check("errors", "ok", "Errors", format!("No read failed in {} reads.", rate(reads as f64)))
    } else if held_errs == 0 {
        check("errors", "info", "Errors", format!("{} of {} reads failed, all in steps past the SLO{}.", rate(errs as f64), rate(reads as f64), top_error.as_ref().map_or(String::new(), |e| format!(" (most: {})", e["error"].as_str().unwrap_or("")))))
    } else {
        check("errors", "bad", "Errors", format!("{} reads failed in steps that held their p99{}.", rate(held_errs as f64), top_error.as_ref().map_or(String::new(), |e| format!(" (most: {})", e["error"].as_str().unwrap_or("")))))
    });
    // Writes.
    if set.write_share > 0.0 {
        let wp99 = busiest.and_then(|b| b["writes"]["p99_ms"].as_f64()).unwrap_or(0.0);
        let at = main_p.at.as_ref().map(|a| a.busy.writer).unwrap_or(0.0);
        checks.push(check("writes", if at > 0.7 { "warn" } else { "ok" }, "Writes", format!("{} of requests were writes; their p99 was {} at the busiest step that held. Each shard's writer is {} busy at {}/s.", pct(set.write_share), ms(wp99), pct(at), rate(at_rate))));
    }
    // The rebuilds after writes.
    if fit.stall_ms > 0.0 && set.write_share > 0.0 {
        // At the rate asked, or the most the cluster holds when that is past it.
        let at = if main_p.capacity_qps.is_finite() && main_p.capacity_qps > 0.0 { at_rate.min(main_p.capacity_qps) } else { at_rate };
        let st = fit.stall(&main_shape, at);
        let mut quiet = fit.clone();
        quiet.stall_ms = 0.0;
        let without = quiet.capacity(&main_shape, slo);
        let with = main_p.capacity_qps;
        let kind = i.write.after_kind.clone().unwrap_or_default();
        let after = i.write.after_insert.map_or(0.0, |q| q.p50);
        let alone = i.stats.iter().find(|k| k.kind == kind).map_or(0.0, |k| k.lat.p50);
        let lost = if with > 0.0 && without.is_finite() { without / with - 1.0 } else { 0.0 };
        let state = if fit.stall_ms >= 0.25 * slo || lost >= 0.15 { "warn" } else { "ok" };
        checks.push(check(
            "stall",
            state,
            "Searches after writes",
            format!(
                "A row added makes the next search on each copy of its shard rebuild the part of the memtable it changed first: {kind} took {} right after an insert, against {} alone. At {}/s, {} of searches wait for one, by the model{}. More copies do not cut it (each copy rebuilds for itself); fewer commits do: rows written in batches.",
                ms(after),
                ms(alone),
                rate(at),
                if st.search_waits < 0.1 { format!("{:.1}%", st.search_waits * 100.0) } else { pct(st.search_waits) },
                if lost >= 0.01 { format!("; without it, the cluster would hold {}/s, {} more", rate(without), pct(lost)) } else { String::new() }
            ),
        ));
    }
    // A kind that costs far more than its share.
    let cpu_total: f64 = fit.kinds.iter().map(|k| k.share * k.cpu_ref * if k.fanout { tested.shards as f64 } else { 1.0 }).sum();
    if cpu_total > 0.0 {
        if let Some((k, share)) = fit.kinds.iter().filter(|k| k.share > 0.0).map(|k| (k, k.share * k.cpu_ref * if k.fanout { tested.shards as f64 } else { 1.0 } / cpu_total)).max_by(|a, b| a.1.total_cmp(&b.1)) {
            if share > 0.3 && share > 2.5 * k.share {
                checks.push(check("heavy", "warn", "A heavy kind", format!("{} is {} of the reads and {} of the shards' work. Cheaper settings for it (a smaller LIMIT, a filter, a cache in front) gain more than any machine.", k.kind, pct(k.share), pct(share))));
            }
        }
    }
    // Cost.
    let base_cost = cost(&base, &m);
    if let Some(c) = base_cost {
        let per_m = if base_p.capacity_qps > 0.0 && base_p.capacity_qps.is_finite() { Some(c / (target.unwrap_or(base_p.capacity_qps * 0.7) * 3600.0 * 24.0 * 30.0) * 1e6) } else { None };
        checks.push(check("cost", "info", "Cost", format!("{} nodes of {} cost about {} a month{}.", nodes(&base), m.name, money(c), per_m.map_or(String::new(), |p| format!(", {} per million reads at {}/s", money_small(p), rate(target.unwrap_or(base_p.capacity_qps * 0.7)))))));
    }

    // ------------------------------------------------ advice
    let advice = advise(fit, &base, &m, &machines, slo, target, at_rate);
    // Over-provisioned: the target holds with a much cheaper shape.
    let cheaper = advice.iter().find(|a| a["id"] == "cheapest").filter(|a| a["cost_delta"].as_f64().is_some_and(|d| base_cost.is_some_and(|c| d < -0.35 * c)));
    if let (Some(c), Some(t)) = (cheaper, target) {
        if cap > t * 2.0 {
            let title = c["title"].as_str().unwrap_or("").replacen("The cheapest", "the cheapest shape", 1);
            checks.push(check("oversized", "warn", "Room to save", format!("It holds {} times your target, and {title}: it {}.", rate(cap / t), c["text"].as_str().unwrap_or(""))));
        }
    }

    // ------------------------------------------------ the grade
    let mut score = 100i64;
    for c in &checks {
        score -= match c["state"].as_str() {
            Some("bad") => 30,
            Some("warn") => 10,
            _ => 0,
        };
    }
    let score = score.clamp(0, 100);
    let bad = checks.iter().any(|c| c["state"] == "bad");
    let label = if checks.iter().any(|c| c["id"] == "capacity" && c["state"] == "bad") {
        "Undersized"
    } else if checks.iter().any(|c| c["id"] == "latency" && c["state"] == "bad") {
        "Too slow"
    } else if checks.iter().any(|c| c["id"] == "oversized") {
        "Oversized"
    } else if bad || score < 60 {
        "At risk"
    } else if score < 85 {
        "Tight"
    } else {
        "Healthy"
    };
    let headline = match (shown_cap, target) {
        (Some(c), Some(t)) => format!("Holds {}{}/s within p99 {} (target {}/s); fills {} first.", if bound { "at least " } else { "" }, rate(c), ms(slo), rate(t), short_part(&first)),
        (Some(c), None) => format!("Holds {}{}/s within p99 {}; fills {} first.", if bound { "at least " } else { "" }, rate(c), ms(slo), short_part(&first)),
        _ => format!("{}/s for {:.0} s against a p99 of {}.", rate(set.config.rate.unwrap_or(0.0)), set.config.seconds, ms(slo)),
    };

    let kinds: Vec<Json> = i
        .stats
        .iter()
        .map(|s| {
            let f = fit.kinds.iter().find(|k| k.kind == s.kind);
            let mut j = serde_json::to_value(s).unwrap_or(Json::Null);
            if let Some(f) = f {
                j["fanout"] = json!(f.fanout);
                j["e"] = json!(f.e);
                // A call's work at twice the rows a shard: how a kind grows.
                j["doubling"] = json!((fit.a0 + f.b * (2.0 * fit.n_ref).powf(f.e)) / f.cpu_ref.max(1e-9));
                j["cpu_call_ms"] = json!(f.cpu_ref);
                j["cpu_share"] = json!(if cpu_total > 0.0 { f.share * f.cpu_ref * if f.fanout { tested.shards as f64 } else { 1.0 } / cpu_total } else { 0.0 });
            }
            j
        })
        .collect();
    let shared_host = fit.tested.copy_cores + 1e-9 < copy_cores_reported;
    let cluster_text = if fit.tested.local {
        "one server".to_string()
    } else {
        format!("{} shards × {} copies, {} coordinator{}", tested.shards, tested.copies, tested.coordinators, if tested.coordinators == 1 { "" } else { "s" })
    };
    json!({
        "config": set.config,
        "settings": {"mix": set.mix, "write_share": set.write_share, "slo_p99_ms": slo, "slo_errors": set.slo_errors, "zipf_s": set.zipf_s, "tail_share": set.tail_share},
        "cluster": {
            "text": cluster_text, "local": fit.tested.local, "shards": tested.shards, "copies": fit.tested.copies, "coordinators": tested.coordinators,
            "copy_cores": fit.tested.copy_cores, "coord_cores": fit.tested.coord_cores, "cores_reported": copy_cores_reported, "shared_host": shared_host,
            "servers": i.servers, "rows_now": i.rows_now,
        },
        "data": i.data,
        "kinds": kinds,
        "write": i.write,
        "steps": i.steps,
        "measured": {"capacity_qps": shown_cap, "bound": bound, "held_qps": held, "failed_qps": failed},
        "fit": fit,
        "model": {
            "kappa": i.kappa,
            "tested": tested_p,
            "main": {"prediction": main_p, "shape": main_shape, "machine": if shared { Some(m.json()) } else { None }, "text": if shared { format!("{} × {} on {} each", tested.shards, tested.copies, m.name) } else { cluster_text.clone() }},
            "curve": curve(fit, &main_shape, 28),
            "base": {"shape": base, "machine": m.json(), "prediction": base_p, "cost_month": base_cost, "docs": docs_est},
            "rate_for_latency": at_rate,
        },
        "grade": {"score": score, "label": label, "headline": headline, "checks": checks},
        "advice": advice,
        "machines": machines,
        "machines_source": REFERENCE_SOURCE,
        "notes": notes(fit, shared_host, docs_est),
    })
}

fn money_small(x: f64) -> String {
    if x >= 10.0 {
        money(x)
    } else {
        format!("${x:.3}")
    }
}

fn short_part(p: &str) -> &'static str {
    match p {
        "coordinator" => "the coordinator",
        "writer" => "the writers",
        _ => "the copies' cores",
    }
}

fn notes(fit: &Fit, shared: bool, docs: f64) -> Vec<String> {
    let mut n = fit.assumptions.clone();
    if shared {
        n.push("the copies of this cluster share machines (or the coordinator's): the test measured them with their machine's cores shared; the shapes predicted give each copy a machine of its own".into());
    }
    let x = docs / fit.tested.docs.max(1) as f64;
    if x > 3.0 {
        n.push(format!("the predictions are for {} rows, {:.0} times the test's: a kind's work at that size is extrapolated from how it grew between the test's two sizes; a test with more documents narrows it", rate(docs), x));
    }
    if (fit.kappa - 1.0).abs() > 1e-6 {
        n.push(format!("the model's cores were scaled by {:.2} to match the capacity the rising rate found", fit.kappa));
    } else if shared {
        n.push("the model is not scaled to the capacity the rising rate found: servers sharing a machine take cores from each other as they need them, which the model of copies on machines of their own does not".into());
    }
    n.push("a copy and a coordinator each have a node of their own; the price is the node's alone (no volumes, no load balancer)".into());
    n
}

/// Shapes worth knowing about, each against `base`.
pub fn advise(fit: &Fit, base: &Shape, m: &Machine, machines: &[Json], slo: f64, target: Option<f64>, at_rate: f64) -> Vec<Json> {
    let b = cell(fit, base, m, slo, at_rate);
    let bcap = b["capacity_qps"].as_f64().unwrap_or(0.0);
    let bp99 = b["p99_ms"].as_f64();
    let bcost = b["cost_month"].as_f64();
    let delta_on = |fit: &Fit, id: &str, title: String, s: Shape, mm: &Machine| -> Json {
        let c = cell(fit, &s, mm, slo, at_rate);
        let cap = c["capacity_qps"].as_f64().unwrap_or(0.0);
        let p99 = c["p99_ms"].as_f64();
        let cost = c["cost_month"].as_f64();
        let dcap = if bcap > 0.0 { cap / bcap - 1.0 } else { 0.0 };
        let dcost = cost.zip(bcost).map(|(a, b)| a - b);
        let mut parts = vec![if dcap.abs() < 0.005 { format!("holds {}/s (the same)", rate(cap)) } else { format!("holds {}/s ({}{})", rate(cap), if dcap >= 0.0 { "+" } else { "−" }, pct(dcap.abs())) }];
        match (p99, bp99) {
            (Some(a), Some(b)) => parts.push(format!("p99 at {}/s {} → {}", rate(at_rate), ms(b), ms(a))),
            (None, Some(_)) => parts.push(format!("cannot hold {}/s", rate(at_rate))),
            _ => {}
        }
        if let Some(d) = dcost {
            parts.push(if d.abs() < 0.5 { "the same cost".into() } else if d > 0.0 { format!("+{} a month", money(d)) } else { format!("{} a month less", money(-d)) });
        }
        json!({
            "id": id, "title": title, "text": parts.join(", "), "shape": {"shards": s.shards, "copies": s.copies, "coordinators": s.coordinators, "machine": mm.name},
            "capacity_qps": cap, "capacity_delta": dcap, "p99_ms": p99, "p99_delta_ms": p99.zip(bp99).map(|(a, b)| a - b), "cost_month": cost, "cost_delta": dcost,
            "holds_target": target.map(|t| cap >= t && p99.is_some_and(|x| x <= slo)), "cell": c,
        })
    };
    let delta = |id: &str, title: String, s: Shape, mm: &Machine| delta_on(fit, id, title, s, mm);
    let mut out = Vec::new();
    out.push(delta("copy_more", format!("A copy more a shard ({} → {})", base.copies, base.copies + 1), Shape { copies: base.copies + 1, ..*base }, m));
    if base.coordinators > 0 {
        out.push(delta("shard_more", format!("A shard more ({} → {})", base.shards, base.shards + 1), Shape { shards: base.shards + 1, ..*base }, m));
        out.push(delta("shards_double", format!("Twice the shards ({} → {})", base.shards, base.shards * 2), Shape { shards: base.shards * 2, ..*base }, m));
    } else {
        out.push(delta("cluster", "A cluster of 2 shards, 1 coordinator".into(), Shape { shards: 2, coordinators: 1, ..*base }, m));
    }
    if base.copies > 1 {
        out.push(delta("copy_less", format!("A copy less a shard ({} → {})", base.copies, base.copies - 1), Shape { copies: base.copies - 1, ..*base }, m));
    }
    if base.shards > 1 {
        let half = (base.shards / 2).max(1);
        out.push(delta("shards_half", format!("Half the shards ({} → {})", base.shards, half), Shape { shards: half, ..*base }, m));
    }
    if base.coordinators > 0 && (b["bottleneck"] == "coordinator" || b["busy"]["coordinator"].as_f64().unwrap_or(0.0) > 0.35) {
        out.push(delta("coord_more", format!("A coordinator more ({} → {})", base.coordinators, base.coordinators + 1), Shape { coordinators: base.coordinators + 1, ..*base }, m));
    }
    let list: Vec<Machine> = machines.iter().filter_map(|x| Machine::of(x).ok()).filter(|x| !x.shared).collect();
    if let Some(up) = list.iter().filter(|x| x.vcpu > m.vcpu).min_by(|a, b| a.vcpu.total_cmp(&b.vcpu).then(a.usd_month.unwrap_or(f64::MAX).total_cmp(&b.usd_month.unwrap_or(f64::MAX)))) {
        out.push(delta("machine_up", format!("Bigger machines ({} → {})", m.name, up.name), on(*base, up), up));
    }
    if let Some(down) = list.iter().filter(|x| x.vcpu < m.vcpu).max_by(|a, b| a.vcpu.total_cmp(&b.vcpu).then(b.usd_month.unwrap_or(f64::MAX).total_cmp(&a.usd_month.unwrap_or(f64::MAX)))) {
        out.push(delta("machine_down", format!("Smaller machines ({} → {})", m.name, down.name), on(*base, down), down));
    }
    // The same rows written ten a request: a tenth of the commits, so of
    // the rebuilds after them.
    if fit.stall_ms > 0.0 && fit.write_share > 0.0 && fit.insert_share > 0.0 {
        let mut batched = fit.clone();
        batched.rows_per_commit = 10.0 * fit.rows_per_commit.max(1.0);
        let mut a = delta_on(&batched, "batch_writes", "Writes in batches of 10 rows".into(), *base, m);
        let gain = a["capacity_delta"].as_f64().unwrap_or(0.0);
        let faster = a["p99_delta_ms"].as_f64().zip(bp99).is_some_and(|(d, b)| d <= -0.1 * b);
        if gain >= 0.05 || faster {
            a["shape"] = Json::Null;
            a["text"] = json!(format!("the same rows a tenth of the commits, so a tenth of the rebuilds: {}", a["text"].as_str().unwrap_or("")));
            out.insert(0, a);
        }
    }
    // The cheapest shape that holds the target with a quarter to spare,
    // two copies a shard at least when the cluster has them.
    if let Some(t) = target {
        let min_copies = base.copies.min(2);
        let mut best: Option<(f64, Shape, Machine)> = None;
        for mm in &list {
            let Some(price) = mm.usd_month else { continue };
            for shards in 1..=16usize {
                for copies in min_copies..=5usize {
                    let s = on(Shape { shards, copies, coordinators: if base.coordinators == 0 && shards == 1 && copies == 1 { 0 } else { base.coordinators.max(1) }, ..*base }, mm);
                    let c = price * nodes(&s) as f64;
                    if best.as_ref().is_some_and(|b| b.0 <= c) {
                        continue;
                    }
                    let p = fit.predict(&s, slo, Some(t));
                    if p.capacity_qps >= 1.25 * t && p.at.as_ref().and_then(|a| a.p99_ms).is_some_and(|x| x <= slo) {
                        best = Some((c, s, mm.clone()));
                    }
                }
            }
        }
        match best {
            Some((_, s, mm)) => {
                let title = format!("The cheapest that holds {}/s with a quarter to spare: {} × {} on {}", rate(t), plural(s.shards, "shard", "shards"), plural(s.copies, "copy", "copies"), mm.name);
                out.insert(0, delta("cheapest", title, s, &mm));
            }
            None => out.insert(0, json!({"id": "cheapest", "title": format!("No shape here holds {}/s", rate(t)), "text": format!("Up to 16 shards × 5 copies on the machines listed, none holds {}/s within a p99 of {}: the model says the work itself is too slow (see Latency at no load).", rate(t), ms(slo)), "shape": null})),
        }
    }
    out
}

/// `POST /v1/admin/simulator/predict`: shapes predicted from a report's model.
pub fn predict(report: &Json, body: &Json) -> Result<Json, ApiError> {
    let fit: Fit = serde_json::from_value(report["fit"].clone()).map_err(|e| ApiError::bad(format!("the report has no model: {e}")))?;
    let slo = body["slo_p99_ms"].as_f64().or_else(|| report["settings"]["slo_p99_ms"].as_f64()).unwrap_or(100.0);
    let machines: Vec<Json> = match body.get("machines") {
        Some(Json::Array(a)) if !a.is_empty() => a.clone(),
        _ => reference_machines(),
    };
    let base_machine = report["model"]["base"]["machine"].clone();
    let m = machine(body.get("machine").filter(|m| !m.is_null()).or(Some(&base_machine)), &machines, fit.tested.copy_cores);
    let docs = body["docs"].as_f64().filter(|d| *d > 0.0).or_else(|| report["model"]["base"]["docs"].as_f64()).unwrap_or(fit.tested.docs as f64);
    let base: Shape = serde_json::from_value(report["model"]["base"]["shape"].clone()).unwrap_or_else(|_| fit.tested_shape());
    let coordinators = body["coordinators"].as_u64().map(|c| c as usize).unwrap_or(base.coordinators);
    let qps = body["qps"].as_f64().filter(|q| *q > 0.0).or_else(|| report["model"]["rate_for_latency"].as_f64()).unwrap_or(100.0);
    let list = |k: &str, d: Vec<usize>| -> Vec<usize> { body[k].as_array().map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| (x as usize).clamp(1, 256)).collect()).filter(|v: &Vec<usize>| !v.is_empty()).unwrap_or(d) };
    let shards = list("shards", (1..=8).collect());
    let copies = list("copies", (1..=4).collect());
    if shards.len() * copies.len() > 400 {
        return Err(ApiError::bad("at most 400 shapes a grid"));
    }
    let shape = |s: usize, r: usize| on(Shape { shards: s, copies: r, coordinators: if coordinators == 0 && (s > 1 || r > 1) { 1 } else { coordinators }, docs, ..base }, &m);
    let cells: Vec<Json> = shards.iter().flat_map(|&s| copies.iter().map(move |&r| (s, r))).map(|(s, r)| cell(&fit, &shape(s, r), &m, slo, qps)).collect();
    let chosen = body.get("shape").and_then(|s| Some((s["shards"].as_u64()? as usize, s["copies"].as_u64()? as usize)));
    let detail = chosen.map(|(s, r)| {
        let sh = shape(s.clamp(1, 256), r.clamp(1, 64));
        let p: Prediction = fit.predict(&sh, slo, Some(qps));
        json!({"cell": cell(&fit, &sh, &m, slo, qps), "prediction": p, "curve": curve(&fit, &sh, 28)})
    });
    let target = body["target_qps"].as_f64().or_else(|| report["config"]["target_qps"].as_f64());
    let base_now = on(Shape { docs, ..base }, &m);
    let advice = advise(&fit, &base_now, &m, &machines, slo, target, qps);
    Ok(json!({"machine": m.json(), "docs": docs, "qps": qps, "slo_p99_ms": slo, "cells": cells, "detail": detail, "base": cell(&fit, &base_now, &m, slo, qps), "base_curve": curve(&fit, &base_now, 28), "advice": advice}))
}
