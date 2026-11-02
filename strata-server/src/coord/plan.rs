//! A plan of a change of a cluster's shape on its own machines
//! (`POST /v1/admin/plan`): the copies each shard is to have, shards to
//! add or remove, slots to move. It changes nothing: it answers the steps
//! (each one an admin call the command center makes in turn), the rows
//! they copy and an estimate of how long, what clients feel meanwhile
//! (`blast`), what to check first (`preflight`) and the copies the plan
//! removes, so that none goes unseen.
//!
//! A plan holds only what was asked: a shard not named keeps its copies,
//! and a shard named keeps those its list names. Copies are added before
//! any is removed, and a primary leaving its shard hands over first, so
//! that no step leaves a shard short of a majority it could keep.

use std::collections::{BTreeSet, HashMap};

use serde_json::{json, Value as Json};

use crate::slots::SLOTS;
use crate::ApiError;

/// Rows a second a copy is sent at, and slots move at, when no move was
/// measured here.
pub const COPY_ROWS_S: f64 = 20_000.0;
pub const MOVE_ROWS_S: f64 = 7_000.0;
/// Seconds each step takes besides its rows: a call, a vote, a flip.
pub const STEP_S: f64 = 5.0;

struct Shard {
    urls: Vec<String>,
    shorts: HashMap<String, String>,
    primary: Option<String>,
    down: Vec<String>,
    docs: u64,
    slots: Vec<(usize, usize)>,
    held: usize,
    failover: bool,
}

fn shards_of(snap: &Json) -> Vec<Shard> {
    snap["shards"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let copies = s["replicas"].as_array().cloned().unwrap_or_default();
            let url = |r: &Json| r["url"].as_str().unwrap_or_default().to_string();
            let primary = s["primary"].as_str().map(String::from);
            // The rows of the shard: its primary's count, else the most any
            // copy counts.
            let docs = copies.iter().find(|r| Some(url(r)) == primary).and_then(|r| r["detail"]["health"]["docs"].as_u64()).or_else(|| copies.iter().filter_map(|r| r["detail"]["health"]["docs"].as_u64()).max()).unwrap_or(0);
            let slots = s["slots"].as_array().into_iter().flatten().filter_map(|r| Some((r[0].as_u64()? as usize, r[1].as_u64()? as usize))).collect();
            Shard {
                urls: copies.iter().map(url).collect(),
                shorts: copies.iter().map(|r| (url(r), r["short"].as_str().unwrap_or_default().to_string())).collect(),
                primary,
                down: copies.iter().filter(|r| r["status"] == "down").map(url).collect(),
                docs,
                slots,
                held: s["slots_held"].as_u64().unwrap_or(0) as usize,
                failover: s["failover"] == true,
            }
        })
        .collect()
}

fn urls(v: &Json, what: &str) -> Result<Vec<String>, ApiError> {
    let list = v.as_array().ok_or_else(|| ApiError::bad(format!("{what} is a list of gRPC URLs (http://host:port)")))?;
    list.iter().map(|u| u.as_str().map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty()).ok_or_else(|| ApiError::bad(format!("{what} is a list of gRPC URLs (http://host:port)")))).collect()
}

/// The plan of `ask` on the cluster `snap` (a `/v1/cluster` snapshot);
/// `moving`: a job of moves runs now.
pub fn plan(snap: &Json, ask: &Json, moving: bool) -> Result<Json, ApiError> {
    plan_at(snap, ask, moving, None)
}

/// [`plan`], with the rows a second the last moves here went at.
pub fn plan_at(snap: &Json, ask: &Json, moving: bool, measured_rows_s: Option<f64>) -> Result<Json, ApiError> {
    let shards = shards_of(snap);
    let n = shards.len();
    let mut steps: Vec<Json> = Vec::new();
    let mut preflight: Vec<Json> = Vec::new();
    let mut removes: Vec<String> = Vec::new();
    let mut blast: Vec<String> = Vec::new();
    let (mut copy_rows, mut move_rows) = (0u64, 0u64);
    let check = |pre: &mut Vec<Json>, ok: bool, text: String| pre.push(json!({"ok": ok, "text": text}));
    let name = |i: usize, url: &str| shards.get(i).and_then(|s| s.shorts.get(url)).cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| url.to_string());
    let all: HashMap<String, usize> = shards.iter().enumerate().flat_map(|(i, s)| s.urls.iter().map(move |u| (u.clone(), i))).collect();
    let remove_shards: BTreeSet<usize> = match ask.get("remove_shards") {
        None | Some(Json::Null) => BTreeSet::new(),
        Some(v) => v.as_array().ok_or_else(|| ApiError::bad("remove_shards is a list of shard numbers"))?.iter().map(|i| i.as_u64().map(|i| i as usize).ok_or_else(|| ApiError::bad("remove_shards is a list of shard numbers"))).collect::<Result<_, _>>()?,
    };
    if let Some(&i) = remove_shards.iter().find(|&&i| i >= n) {
        return Err(ApiError::bad(format!("there is no shard {i} of {n} to remove")));
    }

    // ----- each shard's copies, as asked
    let mut adds: Vec<(usize, String)> = Vec::new();
    let mut handovers: Vec<(usize, String, String)> = Vec::new();
    let mut drops: Vec<(usize, String)> = Vec::new();
    let mut new_shards: Vec<(usize, Vec<String>)> = Vec::new();
    let mut named = BTreeSet::new();
    for s in ask.get("shards").and_then(Json::as_array).into_iter().flatten() {
        let i = s["shard"].as_u64().ok_or_else(|| ApiError::bad("each of shards is {\"shard\": n, \"copies\": [urls]}"))? as usize;
        if !named.insert(i) {
            return Err(ApiError::bad(format!("shard {i} is named twice")));
        }
        let want = urls(&s["copies"], &format!("shard {i}'s copies"))?;
        if want.is_empty() {
            return Err(ApiError::bad(format!("shard {i} keeps one copy at least; to remove the shard, name it in remove_shards")));
        }
        for u in &want {
            if let Some(&j) = all.get(u).filter(|&&j| j != i) {
                check(&mut preflight, false, format!("{u} serves shard {j} already: a server is a copy of one shard"));
            }
        }
        if i >= n {
            new_shards.push((i, want));
            continue;
        }
        let have = &shards[i];
        for u in want.iter().filter(|u| !have.urls.contains(u)) {
            adds.push((i, u.clone()));
        }
        let leaving: Vec<String> = have.urls.iter().filter(|u| !want.contains(u)).cloned().collect();
        for u in &leaving {
            if have.primary.as_deref() == Some(u.as_str()) {
                let to = want.iter().find(|w| have.urls.contains(w) && !have.down.contains(w)).cloned();
                match to {
                    Some(to) => handovers.push((i, u.clone(), to)),
                    None => check(&mut preflight, false, format!("{} is shard {i}'s primary and no copy it keeps is up to hand over to: add the new copies first, in a plan of their own", name(i, u))),
                }
            }
            drops.push((i, u.clone()));
        }
        // What the shard is left with, against what failover needs.
        let left = want.iter().filter(|u| !have.down.contains(u)).count();
        if have.failover && want.len() == 2 {
            check(&mut preflight, true, format!("shard {i} is left with 2 copies: with failover, one down stops its reads and writes (3 keep it serving through one)"));
        }
        if left * 2 <= want.len() {
            check(&mut preflight, false, format!("shard {i} would have {left} of {} copies up: not a majority. Bring its copies back first", want.len()));
        }
    }
    let wanted_new: Vec<usize> = new_shards.iter().map(|(i, _)| *i).collect();
    for (k, i) in wanted_new.iter().enumerate() {
        if *i != n + k {
            return Err(ApiError::bad(format!("a new shard is numbered {}, after the {n} there are", n + k)));
        }
    }
    let add_shards = ask.get("add_shards").and_then(Json::as_u64).unwrap_or(0) as usize;
    if add_shards > new_shards.len() {
        check(&mut preflight, false, format!("{} new shard(s) have no servers: on its own machines a shard is added with its servers' URLs (name them in shards, as shard {} on)", add_shards - new_shards.len(), n + new_shards.len()));
    }

    // ----- shards down or short of copies
    for (i, s) in shards.iter().enumerate() {
        if !s.down.is_empty() && !named.contains(&i) && !remove_shards.contains(&i) {
            let d: Vec<String> = s.down.iter().map(|u| name(i, u)).collect();
            check(&mut preflight, true, format!("shard {i} has {} down; the plan leaves it so", d.join(", ")));
        }
    }

    // ----- the steps: repairs and additions first, then handovers, then removals
    for (i, u) in &adds {
        copy_rows += shards[*i].docs;
        steps.push(json!({"kind": "add_copy", "shard": i, "copy": u, "text": format!("Add {u} to shard {i}: it is sent a copy of the shard (about {} rows) and joins it", shards[*i].docs),
            "call": {"method": "POST", "path": format!("/v1/admin/shards/{i}/replicas"), "body": {"url": u}}}));
    }
    for (i, from, to) in &handovers {
        steps.push(json!({"kind": "handover", "shard": i, "copy": from, "text": format!("Hand shard {i}'s primary over from {} to {}", name(*i, from), name(*i, to)),
            "call": {"method": "POST", "path": format!("/v1/admin/shards/{i}/promote"), "body": {"url": to}}}));
        blast.push(format!("writes to shard {i} wait for its new primary, about a second"));
    }
    for (i, u) in &drops {
        removes.push(u.clone());
        steps.push(json!({"kind": "remove_copy", "shard": i, "copy": u, "text": format!("Remove {} from shard {i}", name(*i, u)),
            "call": {"method": "DELETE", "path": format!("/v1/admin/shards/{i}/replicas?url={u}"), "body": null}}));
    }
    for (i, copies) in &new_shards {
        steps.push(json!({"kind": "add_shard", "shard": i, "text": format!("Add shard {i}, served by {}; it holds no slots until some move to it", copies.join(", ")),
            "call": {"method": "POST", "path": "/v1/admin/shards", "body": {"servers": copies}}}));
    }

    // ----- slots moved
    let owner = |slot: usize| shards.iter().position(|s| s.slots.iter().any(|&(a, b)| slot >= a && slot < b));
    let total = n + new_shards.len();
    for m in ask.get("moves").and_then(Json::as_array).into_iter().flatten() {
        let (a, b) = match m["slots"].as_array().map(|r| (r.first().and_then(Json::as_u64), r.get(1).and_then(Json::as_u64))) {
            Some((Some(a), Some(b))) if a < b && b as usize <= SLOTS => (a as usize, b as usize),
            _ => return Err(ApiError::bad(format!("each move's slots is [start, end) within 0..{SLOTS}"))),
        };
        let to = m["to"].as_u64().ok_or_else(|| ApiError::bad("each move names the shard it goes \"to\""))? as usize;
        if to >= total || remove_shards.contains(&to) {
            return Err(ApiError::bad(format!("slots move to shard {to}, which the plan does not keep")));
        }
        let from = owner(a);
        if (a..b).any(|s| owner(s) != from) {
            return Err(ApiError::bad(format!("slots {a}..{b} are held by more than one shard: a move is of one shard's slots")));
        }
        let Some(from) = from else { return Err(ApiError::bad(format!("no shard holds slots {a}..{b}"))) };
        if from == to {
            return Err(ApiError::bad(format!("slots {a}..{b} are shard {to}'s already")));
        }
        let rows = (shards[from].docs as f64 * (b - a) as f64 / shards[from].held.max(1) as f64).round() as u64;
        move_rows += rows;
        steps.push(json!({"kind": "move", "shard": from, "text": format!("Move slots {a}..{b} ({} slots, about {rows} rows) from shard {from} to shard {to}", b - a),
            "call": {"method": "POST", "path": "/v1/admin/moves", "body": {"slots": [[a, b]], "to": to}}, "wait": true}));
    }
    if ask.get("moves").and_then(Json::as_array).is_some_and(|v| !v.is_empty()) {
        blast.push("writes to the slots moving wait at each flip, under a second".into());
    }

    // ----- shards removed: drained, then removed with their copies
    if !remove_shards.is_empty() && remove_shards.len() >= total {
        check(&mut preflight, false, "the plan removes every shard: a cluster keeps one".into());
    }
    for &i in remove_shards.iter().rev() {
        move_rows += shards[i].docs;
        steps.push(json!({"kind": "drain", "shard": i, "text": format!("Drain shard {i}: its {} slots (about {} rows) move to the other shards", shards[i].held, shards[i].docs),
            "call": {"method": "POST", "path": format!("/v1/admin/shards/{i}/drain"), "body": {}}, "wait": true}));
        let copies: Vec<String> = shards[i].urls.iter().map(|u| name(i, u)).collect();
        steps.push(json!({"kind": "remove_shard", "shard": i, "text": format!("Remove shard {i} and its copies ({})", copies.join(", ")),
            "call": {"method": "DELETE", "path": format!("/v1/admin/shards/{i}"), "body": null}}));
        removes.extend(shards[i].urls.iter().cloned());
    }
    if !remove_shards.is_empty() {
        blast.push("the drained shards' rows move while the cluster serves; writes to each run of slots wait at its flip".into());
    }
    if moving && steps.iter().any(|s| s["wait"] == true) {
        check(&mut preflight, false, "a job of moves runs now: wait for it, or stop it, before moving more slots".into());
    }
    if steps.is_empty() {
        check(&mut preflight, true, "the plan changes nothing: the cluster has this shape already".into());
    }
    if preflight.iter().all(|p| p["ok"] == true) && !steps.is_empty() {
        check(&mut preflight, true, "every copy the plan keeps or adds is named once".into());
    }
    let move_rate = measured_rows_s.filter(|r| *r > 0.0).unwrap_or(MOVE_ROWS_S);
    let estimate = copy_rows as f64 / COPY_ROWS_S + move_rows as f64 / move_rate + steps.len() as f64 * STEP_S;
    if blast.is_empty() {
        blast.push(if steps.is_empty() { "nothing changes".into() } else { "searches and writes go on throughout".into() });
    } else {
        blast.insert(0, "searches go on throughout".into());
    }
    let mut blast = blast.join("; ");
    if let Some(c) = blast.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    Ok(json!({
        "steps": steps, "rows_to_copy": copy_rows + move_rows, "estimate_s": estimate.round(),
        "estimate_from": format!("{COPY_ROWS_S} rows/s to a new copy, {move_rate:.0} rows/s for moves{}, {STEP_S} s a step", if measured_rows_s.is_some() { " (measured here)" } else { "" }),
        "blast": blast + ".", "preflight": preflight, "removes_copies": removes,
        "ok": preflight.iter().all(|p| p["ok"] == true),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Json {
        let copy = |i: usize, k: usize, up: bool| json!({"url": format!("http://h{i}{k}"), "short": format!("s{i}-{k}"), "status": if up { "ok" } else { "down" }, "detail": {"health": {"docs": 6000}}});
        json!({"shards": [
            {"shard": 0, "primary": "http://h00", "failover": true, "slots": [[0, 2048]], "slots_held": 2048, "replicas": [copy(0, 0, true), copy(0, 1, true), copy(0, 2, true)]},
            {"shard": 1, "primary": "http://h10", "failover": true, "slots": [[2048, 4096]], "slots_held": 2048, "replicas": [copy(1, 0, true), copy(1, 1, true)]},
        ]})
    }

    #[test]
    fn a_plan_holds_only_what_was_asked() {
        // Removing shard 1 leaves shard 0's three copies alone (CC-1).
        let p = plan(&snap(), &json!({"remove_shards": [1]}), false).unwrap();
        let kinds: Vec<&str> = p["steps"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["drain", "remove_shard"]);
        assert_eq!(p["removes_copies"], json!(["http://h10", "http://h11"]));
        assert_eq!(p["rows_to_copy"], 6000);
        assert_eq!(p["ok"], true, "{p}");
    }

    #[test]
    fn copies_are_added_before_a_primary_hands_over_and_leaves() {
        let ask = json!({"shards": [{"shard": 1, "copies": ["http://h11", "http://new"]}]});
        let p = plan(&snap(), &ask, false).unwrap();
        let kinds: Vec<&str> = p["steps"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["add_copy", "handover", "remove_copy"]);
        assert_eq!(p["steps"][1]["call"]["body"]["url"], "http://h11");
        assert_eq!(p["removes_copies"], json!(["http://h10"]));
        assert!(p["blast"].as_str().unwrap().starts_with("Searches go on throughout; writes to shard 1 wait"), "{p}");
    }

    #[test]
    fn a_copy_of_another_shard_and_a_running_job_fail_the_preflight() {
        let p = plan(&snap(), &json!({"shards": [{"shard": 1, "copies": ["http://h10", "http://h11", "http://h00"]}]}), false).unwrap();
        assert_eq!(p["ok"], false);
        let p = plan(&snap(), &json!({"moves": [{"slots": [0, 100], "to": 1}]}), true).unwrap();
        assert_eq!(p["ok"], false);
        assert_eq!(p["steps"][0]["text"], "Move slots 0..100 (100 slots, about 293 rows) from shard 0 to shard 1");
        assert!(plan(&snap(), &json!({"moves": [{"slots": [2000, 2100], "to": 1}]}), false).is_err(), "two shards' slots");
    }
}
