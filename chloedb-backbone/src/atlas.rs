//! RIPE Atlas results into rows: a ping (`bb_ping`) per probe and round,
//! and a traceroute (`bb_trace`) per probe and round, with the networks its
//! hops belong to and where its time jumps (the hop after which the
//! packets crossed an ocean, or a slow link).

use serde_json::{json, Value};

use crate::load::Probes;
use crate::mrt::Prefix;
use crate::refdata::Reference;

fn iso(s: i64) -> Value {
    chrono::DateTime::from_timestamp(s, 0).map(|d| json!(d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))).unwrap_or(Value::Null)
}

/// The network an address is announced by, as RouteViews saw it.
pub fn asn_of(r: &Reference, ip: &str) -> Option<u32> {
    let p = if ip.contains(':') { Prefix::parse(&format!("{ip}/128"))? } else { Prefix::parse(&format!("{ip}/32"))? };
    r.usual.covering(&p).next().and_then(|(_, v)| v.first().copied())
}

pub fn rows(results: &[Value], probes: &Probes, r: &Reference) -> (Vec<Value>, Vec<Value>) {
    let mut pings = Vec::new();
    let mut traces = Vec::new();
    for x in results {
        let Some(prb) = x["prb_id"].as_u64() else { continue };
        let msm = x["msm_id"].as_u64().unwrap_or(0);
        let t = x["timestamp"].as_i64().unwrap_or(0);
        let (asn, cc) = probes.get(&prb).cloned().unwrap_or_default();
        let target = x["dst_name"].as_str().or(x["dst_addr"].as_str()).unwrap_or("").to_string();
        match x["type"].as_str() {
            Some("ping") => {
                let sent = x["sent"].as_f64().unwrap_or(0.0);
                let rcvd = x["rcvd"].as_f64().unwrap_or(0.0);
                let min = x["min"].as_f64().filter(|m| *m >= 0.0);
                let avg = x["avg"].as_f64().filter(|m| *m >= 0.0);
                pings.push(json!({
                    "id": format!("{msm}-{prb}-{t}"), "ts": iso(t), "msm": msm, "target": target, "probe": prb,
                    "asn": asn, "country": cc, "rtt": min, "avg": avg,
                    "loss": if sent > 0.0 { 1.0 - rcvd / sent } else { 1.0 },
                }));
            }
            Some("traceroute") => {
                let dst = x["dst_addr"].as_str().unwrap_or("");
                let mut hops: Vec<(i64, String, f64)> = Vec::new();
                let mut last_hop = 0;
                for h in x["result"].as_array().into_iter().flatten() {
                    let n = h["hop"].as_i64().unwrap_or(0);
                    last_hop = last_hop.max(n);
                    let mut best: Option<(String, f64)> = None;
                    for a in h["result"].as_array().into_iter().flatten() {
                        if let (Some(from), Some(rtt)) = (a["from"].as_str(), a["rtt"].as_f64()) {
                            if best.as_ref().is_none_or(|b| rtt < b.1) {
                                best = Some((from.to_string(), rtt));
                            }
                        }
                    }
                    if let Some((ip, rtt)) = best {
                        hops.push((n, ip, rtt));
                    }
                }
                let reached = hops.iter().filter(|h| h.1 == dst).map(|h| h.2).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v))));
                // The largest rise from one answering hop to the next.
                let mut jump: Option<(i64, f64, String, String)> = None;
                for w in hops.windows(2) {
                    let d = w[1].2 - w[0].2;
                    if jump.as_ref().is_none_or(|j| d > j.1) {
                        jump = Some((w[1].0, d, w[0].1.clone(), w[1].1.clone()));
                    }
                }
                let mut path_as: Vec<u32> = Vec::new();
                for (_, ip, _) in &hops {
                    if let Some(a) = asn_of(r, ip) {
                        if path_as.last() != Some(&a) {
                            path_as.push(a);
                        }
                    }
                }
                let route: Vec<String> = hops.iter().map(|(n, ip, rtt)| format!("{n} {ip} {rtt:.1}")).collect();
                traces.push(json!({
                    "id": format!("{msm}-{prb}-{t}"), "ts": iso(t), "msm": msm, "target": target, "probe": prb,
                    "asn": asn, "country": cc, "hops": last_hop, "rtt": reached,
                    "jump_hop": jump.as_ref().map(|j| j.0), "jump_ms": jump.as_ref().map(|j| (j.1 * 10.0).round() / 10.0),
                    "jump_from": jump.as_ref().map(|j| j.2.clone()), "jump_to": jump.as_ref().map(|j| j.3.clone()),
                    "jump_as": jump.as_ref().and_then(|j| asn_of(r, &j.3)), "before_as": jump.as_ref().and_then(|j| asn_of(r, &j.2)),
                    "ips": hops.iter().map(|h| h.1.clone()).collect::<Vec<_>>(), "path_as": path_as,
                    "route": route.join("|"),
                }));
            }
            _ => {}
        }
    }
    (pings, traces)
}
