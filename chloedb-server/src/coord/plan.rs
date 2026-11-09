//! A plan of a change of a cluster's shape on its own machines
//! (`POST /v1/admin/plan`): the copies each shard is to have, shards to
//! add or remove, slots to move. It changes nothing: it answers the steps
//! (each one an admin call), the rows they copy (of every table) and an
//! estimate of how long, what clients feel meanwhile (`blast`), what to
//! check first (`preflight`, the disks' room among it) and the copies the
//! plan removes, so that none goes unseen.
//!
//! A plan holds only what was asked: a shard not named keeps its copies,
//! and a shard named keeps those its list names. Copies are added before
//! any is removed, and a primary leaving its shard hands over first, so
//! that no step leaves a shard short of a majority it could keep.
//!
//! A plan is applied here, on the coordinator (`POST /v1/admin/plan/apply`,
//! [`apply`]): its steps run in order in the background, as a job of moves
//! does, whoever closes their browser, and every view and coordinator can
//! follow it (`GET /v1/admin/plan/run`). The plan is kept in a file (next
//! to the audit log) after every step, so that a coordinator that restarts
//! mid-plan says where it stopped, and can resume it.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use super::{Coordinator, MOVE_SLOTS};
use crate::service::Service;
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
    /// With no primary that answers: the copy down that was the primary
    /// when it last answered, which the coordinator still takes for it.
    was_primary: Option<String>,
    /// The copies that answer, of the latest term and transaction first:
    /// those the others vote for.
    newest: Vec<String>,
    down: Vec<String>,
    /// Rows of every table (a copy's `rows`; `docs` on an older server,
    /// which counts the first table only).
    docs: u64,
    /// Bytes of the database on its primary (file and log), about what a
    /// new copy is sent.
    bytes: Option<u64>,
    slots: Vec<(usize, usize)>,
    held: usize,
    failover: bool,
    /// Each copy that answers and reports its volume.
    disks: Vec<Disk>,
}

/// A copy's volume, as its last point says.
#[derive(Clone)]
struct Disk {
    url: String,
    short: String,
    used: f64,
    total: f64,
    free: f64,
}

/// A copy's rows: of every table, else the first's (an older server).
pub fn copy_rows(r: &Json) -> Option<u64> {
    r["detail"]["health"]["rows"].as_u64().or_else(|| r["detail"]["health"]["docs"].as_u64()).or_else(|| r["docs"].as_u64())
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
            let docs = copies.iter().find(|r| Some(url(r)) == primary).and_then(copy_rows).or_else(|| copies.iter().filter_map(copy_rows).max()).unwrap_or(0);
            let bytes_of = |r: &Json| {
                let l = &r["detail"]["load"];
                l["file_bytes"].as_f64().map(|f| (f + l["wal_bytes"].as_f64().unwrap_or(0.0)) as u64)
            };
            let bytes = copies.iter().find(|r| Some(url(r)) == primary).and_then(bytes_of).or_else(|| copies.iter().filter_map(bytes_of).max());
            let disks = copies
                .iter()
                .filter(|r| r["status"] != "down")
                .filter_map(|r| {
                    let l = &r["detail"]["load"];
                    let (total, free) = (l["disk_total_bytes"].as_f64()?, l["disk_free_bytes"].as_f64()?);
                    (total > 0.0).then(|| Disk { url: url(r), short: r["short"].as_str().unwrap_or_default().to_string(), used: 1.0 - free / total, total, free })
                })
                .collect();
            let slots = s["slots"].as_array().into_iter().flatten().filter_map(|r| Some((r[0].as_u64()? as usize, r[1].as_u64()? as usize))).collect();
            let was_primary = match primary {
                Some(_) => None,
                None => copies.iter().filter(|r| r["status"] == "down" && r["last_role"] == "primary").max_by_key(|r| r["term"].as_u64().unwrap_or(0)).map(url),
            };
            let mut live: Vec<&Json> = copies.iter().filter(|r| r["status"] != "down").collect();
            live.sort_by_key(|r| std::cmp::Reverse((r["term"].as_u64().unwrap_or(0), r["txn"].as_u64().unwrap_or(0))));
            Shard {
                urls: copies.iter().map(url).collect(),
                shorts: copies.iter().map(|r| (url(r), r["short"].as_str().unwrap_or_default().to_string())).collect(),
                primary,
                was_primary,
                newest: live.into_iter().map(url).collect(),
                down: copies.iter().filter(|r| r["status"] == "down").map(url).collect(),
                docs,
                bytes,
                slots,
                held: s["slots_held"].as_u64().unwrap_or(0) as usize,
                failover: s["failover"] == true,
                disks,
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
    // Bytes each copy of a shard gains, and the servers new to the cluster
    // with the bytes each is sent (for the disk checks).
    let mut gain: HashMap<usize, f64> = HashMap::new();
    let mut new_servers: Vec<(String, Option<u64>)> = Vec::new();
    let mut disks: Vec<Json> = Vec::new();
    let per_row = |i: usize| shards.get(i).and_then(|s| s.bytes.map(|b| b as f64 / s.docs.max(1) as f64)).unwrap_or(0.0);
    // A check's level: ok, warn (advice: the plan may go on) or fail; `ok`
    // is false for a failed one only.
    let check = |pre: &mut Vec<Json>, level: &str, text: String| pre.push(json!({"ok": level != "fail", "level": level, "text": text}));
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
                check(&mut preflight, "fail", format!("{u} serves shard {j} already: a server is a copy of one shard"));
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
        // A lone copy is the primary once a copy joins it. A dead primary
        // is still the primary to the coordinator, which removes it only
        // once another copy is (finding chaos-2).
        let primary = have.primary.clone().or_else(|| have.was_primary.clone()).or_else(|| (have.urls.len() == 1).then(|| have.urls[0].clone()));
        for u in &leaving {
            if primary.as_deref() == Some(u.as_str()) {
                // A copy it keeps that is up (for a dead primary, the one the
                // others vote for); else one this plan adds, which is in sync
                // before the handover (additions run first).
                let kept_up = |w: &&String| want.contains(*w) && !have.down.contains(*w);
                let to = match have.primary.is_none() {
                    true => have.newest.iter().find(kept_up),
                    false => want.iter().find(|w| have.urls.contains(w) && !have.down.contains(w)),
                }
                .or_else(|| want.iter().find(|w| !have.urls.contains(w)))
                .cloned();
                match to {
                    Some(to) => handovers.push((i, u.clone(), to)),
                    None => check(&mut preflight, "fail", format!("{} is shard {i}'s primary and no copy it keeps is up to hand over to: add a copy first", name(i, u))),
                }
            }
            drops.push((i, u.clone()));
        }
        // What the shard is left with, against what failover needs.
        let left = want.iter().filter(|u| !have.down.contains(u)).count();
        if have.failover && want.len() == 2 {
            check(&mut preflight, "warn", format!("shard {i} is left with 2 copies: with failover, one down stops its reads and writes (3 keep it serving through one)"));
        }
        if left * 2 <= want.len() {
            check(&mut preflight, "fail", format!("shard {i} would have {left} of {} copies up: not a majority. Bring its copies back first", want.len()));
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
        check(&mut preflight, "fail", format!("{} new shard(s) have no servers: on its own machines a shard is added with its servers' URLs (name them in shards, as shard {} on)", add_shards - new_shards.len(), n + new_shards.len()));
    }

    // ----- shards down or short of copies
    for (i, s) in shards.iter().enumerate() {
        if !s.down.is_empty() && !named.contains(&i) && !remove_shards.contains(&i) {
            let d: Vec<String> = s.down.iter().map(|u| name(i, u)).collect();
            check(&mut preflight, "warn", format!("shard {i} has {} down; the plan leaves it so", d.join(", ")));
        }
    }

    // ----- the steps: repairs and additions first, then handovers, then removals
    for (i, u) in &adds {
        copy_rows += shards[*i].docs;
        new_servers.push((u.clone(), shards[*i].bytes));
        steps.push(json!({"kind": "add_copy", "shard": i, "copy": u, "url": u, "rows": shards[*i].docs, "text": format!("Add {u} to shard {i}: it is sent a copy of the shard (about {} rows) and joins it", thousands(shards[*i].docs as f64)),
            "call": {"method": "POST", "path": format!("/v1/admin/shards/{i}/replicas"), "body": {"url": u}}}));
    }
    for (i, from, to) in &handovers {
        // From a dead primary: the votes of the copies that answer, skipped
        // when they have elected a primary by then (a repair).
        let dead = shards[*i].down.contains(from);
        let text = match dead {
            true => format!("Make {} shard {i}'s primary: {}, its primary, does not answer", name(*i, to), name(*i, from)),
            false => format!("Hand shard {i}'s primary over from {} to {}", name(*i, from), name(*i, to)),
        };
        steps.push(json!({"kind": "handover", "shard": i, "copy": from, "from": from, "url": to, "text": text, "repair": dead,
            "call": {"method": "POST", "path": format!("/v1/admin/shards/{i}/promote"), "body": {"url": to}}}));
        blast.push(format!("writes to shard {i} wait for its new primary, about a second"));
    }
    for (i, u) in &drops {
        removes.push(u.clone());
        steps.push(json!({"kind": "remove_copy", "shard": i, "copy": u, "url": u, "text": format!("Remove {} from shard {i}", name(*i, u)),
            "call": {"method": "DELETE", "path": format!("/v1/admin/shards/{i}/replicas?url={u}"), "body": null}}));
    }
    for (i, copies) in &new_shards {
        new_servers.extend(copies.iter().map(|u| (u.clone(), None)));
        steps.push(json!({"kind": "add_shard", "shard": i, "servers": copies, "text": format!("Add shard {i}, served by {}; it holds no slots until some move to it", copies.join(", ")),
            "call": {"method": "POST", "path": "/v1/admin/shards", "body": {"servers": copies}}}));
        // A new shard of 2 copies is what a failover cluster loses first.
        let failover = shards.iter().any(|s| s.failover);
        match copies.len() {
            1 if n > 0 => check(&mut preflight, "warn", format!("new shard {i} has 1 copy: losing its server loses the shard until it is back (3 keep it serving through one)")),
            2 if failover => check(&mut preflight, "warn", format!("new shard {i} has 2 copies: with failover, losing either stops its reads and writes; use 3")),
            _ => {}
        }
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
        *gain.entry(to).or_default() += rows as f64 * per_row(from);
        steps.push(json!({"kind": "move", "shard": from, "slots": [[a, b]], "to": to, "rows": rows, "text": format!("Move slots {a}..{b} ({} slots, about {} rows) from shard {from} to shard {to}", b - a, thousands(rows as f64)),
            "call": {"method": "POST", "path": "/v1/admin/moves", "body": {"slots": [[a, b]], "to": to}}, "wait": true}));
    }
    if ask.get("moves").and_then(Json::as_array).is_some_and(|v| !v.is_empty()) {
        blast.push("writes to the slots moving wait at each flip, under a second".into());
    }

    // ----- shards removed: drained, then removed with their copies
    if !remove_shards.is_empty() && remove_shards.len() >= total {
        check(&mut preflight, "fail", "the plan removes every shard: a cluster keeps one".into());
    }
    // A drain evens every other shard out too ([`super::plan_even`]): the
    // slots each holds after the plan's moves, and the rows they hold.
    let mut held: Vec<usize> = shards.iter().map(|s| s.held).collect();
    let mut rows: Vec<f64> = shards.iter().map(|s| s.docs as f64).collect();
    held.resize(total, 0);
    rows.resize(total, 0.0);
    for m in steps.iter().filter(|s| s["kind"] == "move") {
        let (from, to) = (m["shard"].as_u64().unwrap_or(0) as usize, m["to"].as_u64().unwrap_or(0) as usize);
        let k = m["slots"][0][1].as_u64().unwrap_or(0).saturating_sub(m["slots"][0][0].as_u64().unwrap_or(0)) as usize;
        let r = m["rows"].as_f64().unwrap_or(0.0);
        held[from] = held[from].saturating_sub(k);
        held[to] += k;
        rows[from] = (rows[from] - r).max(0.0);
        rows[to] += r;
    }
    let mut gone = BTreeSet::new();
    // `"even": true`: the slots even out over every shard after the moves
    // (new shards take their share), as `POST /v1/admin/rebalance` does.
    if ask.get("even").and_then(Json::as_bool) == Some(true) {
        let (mut slots, mut sent, mut to_new) = (0usize, 0.0, 0.0);
        for (from, to, k) in even_counts(&held, &gone, None) {
            let r = rows[from] * k as f64 / held[from].max(1) as f64;
            *gain.entry(to).or_default() += r * per_row(from);
            held[from] -= k;
            held[to] += k;
            rows[from] -= r;
            rows[to] += r;
            slots += k;
            sent += r;
            if to >= n {
                to_new += r;
            }
        }
        if slots > 0 {
            let sent = sent.round() as u64;
            move_rows += sent;
            let shares = total - remove_shards.len();
            steps.push(json!({"kind": "rebalance", "rows": sent, "text": format!("Even the slots out over {shares} shards: about {} slots and {} rows move{}, up to {MOVE_SLOTS} slots at a time", thousands(slots as f64), thousands(sent as f64), {
                let new = if new_shards.len() == 1 { "shard" } else { "shards" };
                let to_new = to_new.round() as u64;
                if to_new == 0 || new_shards.is_empty() { String::new() } else if to_new == sent { format!(" to the new {new}") } else { format!(" ({} of them to the new {new})", thousands(to_new as f64)) }
            }),
                "call": {"method": "POST", "path": "/v1/admin/rebalance", "body": {}}, "wait": true}));
            blast.push("writes to each run of slots that moves wait at its flip, under a second".into());
        }
    }
    // A new server is sent its shard's share of the rows that move.
    for (i, copies) in &new_shards {
        let b = gain.get(i).map(|b| b.round() as u64);
        for (u, bytes) in new_servers.iter_mut() {
            if copies.contains(u) {
                *bytes = b;
            }
        }
    }
    for &i in remove_shards.iter().rev() {
        let mut drained = 0.0;
        for (from, to, k) in even_counts(&held, &gone, Some(i)) {
            let r = rows[from] * k as f64 / held[from].max(1) as f64;
            *gain.entry(to).or_default() += r * per_row(from);
            held[from] -= k;
            held[to] += k;
            rows[from] -= r;
            rows[to] += r;
            drained += r;
        }
        gone.insert(i);
        let drained = drained.round() as u64;
        move_rows += drained;
        let others = drained.saturating_sub(shards[i].docs);
        steps.push(json!({"kind": "drain", "shard": i, "rows": drained, "text": format!("Drain shard {i}: its {} slots (about {} rows) move to the other shards{}", thousands(shards[i].held as f64), thousands(shards[i].docs as f64), if others > 0 { format!(", which even out among themselves too (about {} rows more)", thousands(others as f64)) } else { String::new() }),
            "call": {"method": "POST", "path": format!("/v1/admin/shards/{i}/drain"), "body": {}}, "wait": true}));
        let copies: Vec<String> = shards[i].urls.iter().map(|u| name(i, u)).collect();
        steps.push(json!({"kind": "remove_shard", "shard": i, "text": format!("Remove shard {i} and its copies ({})", copies.join(", ")),
            "call": {"method": "DELETE", "path": format!("/v1/admin/shards/{i}"), "body": null}}));
        removes.extend(shards[i].urls.iter().cloned());
    }
    if !remove_shards.is_empty() {
        blast.push("the drained shards' rows move while the cluster serves; writes to each run of slots wait at its flip".into());
    }
    let slots_after: Vec<Json> = (0..total).filter(|i| !gone.contains(i)).map(|i| json!({"shard": i, "slots": held[i], "rows": rows[i].round()})).collect();
    disk_checks(&shards, &gain, &remove_shards, &mut disks, &mut preflight);
    if moving && steps.iter().any(|s| s["wait"] == true) {
        check(&mut preflight, "fail", "a job of moves runs now: wait for it, or stop it, before moving more slots".into());
    }
    if steps.is_empty() {
        check(&mut preflight, "ok", "the plan changes nothing: the cluster has this shape already".into());
    }
    if preflight.iter().all(|p| p["ok"] == true) && !steps.is_empty() {
        check(&mut preflight, "ok", "every copy the plan keeps or adds is named once".into());
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
        "estimate_from": format!(
            "About {} rows a second to a new copy, {} rows a second for moves{}, and {STEP_S} s a step.",
            thousands(COPY_ROWS_S),
            thousands(move_rate),
            if measured_rows_s.is_some() { " (as measured here)" } else { "" }
        ),
        "blast": blast + ".", "preflight": preflight, "removes_copies": removes,
        "disks": disks, "after": slots_after,
        "new_servers": new_servers.iter().map(|(u, b)| json!({"url": u, "bytes": b})).collect::<Vec<_>>(),
        "ok": preflight.iter().all(|p| p["ok"] == true),
    }))
}

/// The moves [`super::plan_even`] makes, by counts of slots only: `(from,
/// to, slots)`, each of at most [`MOVE_SLOTS`], leaving every shard but
/// `empty` and those `gone` an even share and `empty` none.
fn even_counts(held: &[usize], gone: &BTreeSet<usize>, empty: Option<usize>) -> Vec<(usize, usize, usize)> {
    let n = held.len();
    let mut count = held.to_vec();
    let mut takers: Vec<usize> = (0..n).filter(|&i| Some(i) != empty && !gone.contains(&i)).collect();
    if takers.is_empty() {
        return Vec::new();
    }
    takers.sort_by_key(|&i| (std::cmp::Reverse(count[i]), i));
    let mut target = vec![0usize; n];
    for (k, &i) in takers.iter().enumerate() {
        target[i] = SLOTS / takers.len() + usize::from(k < SLOTS % takers.len());
    }
    let mut out = Vec::new();
    loop {
        let donor = (0..n).filter(|&i| count[i] > target[i]).max_by_key(|&i| (count[i] - target[i], std::cmp::Reverse(i)));
        let taker = (0..n).filter(|&i| count[i] < target[i]).max_by_key(|&i| (target[i] - count[i], std::cmp::Reverse(i)));
        let (Some(d), Some(r)) = (donor, taker) else { break };
        let k = (count[d] - target[d]).min(target[r] - count[r]).min(MOVE_SLOTS);
        count[d] -= k;
        count[r] += k;
        out.push((d, r, k));
    }
    out
}

/// The disk checks of a plan: each copy it writes to with its volume now
/// and after (`disks`), failed when what it is sent does not fit, a
/// warning past 85% full; a copy it leaves alone that is nearly full is a
/// warning too, so that no check is green while a disk is at 97%.
fn disk_checks(shards: &[Shard], gain: &HashMap<usize, f64>, removed: &BTreeSet<usize>, disks: &mut Vec<Json>, pre: &mut Vec<Json>) {
    let check = |pre: &mut Vec<Json>, level: &str, text: String| pre.push(json!({"ok": level != "fail", "level": level, "text": text}));
    let pct = |r: f64| format!("{:.0}%", r * 100.0);
    let mut worst: Option<(String, f64)> = None;
    let mut reported = false;
    // Copies it sends nothing that are nearly full, by how full: one line each.
    let mut idle_full: std::collections::BTreeMap<u64, Vec<String>> = std::collections::BTreeMap::new();
    for (i, s) in shards.iter().enumerate() {
        if removed.contains(&i) {
            continue;
        }
        let add = gain.get(&i).copied().unwrap_or(0.0);
        for d in &s.disks {
            reported = true;
            let name = if d.short.is_empty() { d.url.clone() } else { d.short.clone() };
            if add > 0.0 {
                let after = (d.total - d.free + add) / d.total;
                disks.push(json!({"shard": i, "copy": d.short, "url": d.url, "now": (d.used * 1000.0).round() / 1000.0, "after": (after * 1000.0).round() / 1000.0, "adds_bytes": add.round()}));
                if add >= d.free * 0.98 || after >= 0.98 {
                    check(pre, "fail", format!("{name}'s disk would be {} full after the plan ({} now, about {} more): free space, grow its volume or add shards with room first", pct(after.min(1.0)), pct(d.used), bytes(add)));
                } else if after >= 0.85 {
                    check(pre, "warn", format!("{name}'s disk would be {} full after the plan ({} now): it has {} free", pct(after), pct(d.used), bytes(d.free - add)));
                }
                if worst.as_ref().is_none_or(|(_, w)| after > *w) {
                    worst = Some((name, after));
                }
            } else if d.used >= 0.85 {
                idle_full.entry((d.used * 100.0).round() as u64).or_default().push(name);
            }
        }
    }
    for (p, names) in idle_full.iter().rev() {
        let said = match names.as_slice() {
            [one] => format!("{one}'s disk is {p}% full now (the plan sends it nothing)"),
            [rest @ .., last] => format!("the disks of {} and {last} are {p}% full now (the plan sends them nothing)", rest.join(", ")),
            [] => continue,
        };
        check(pre, "warn", said);
    }
    if !reported {
        check(pre, "warn", "the copies do not report their disks: the plan cannot check their room".into());
    } else if let Some((name, after)) = worst.filter(|(_, a)| *a < 0.85) {
        check(pre, "ok", format!("the disks have room: the fullest copy the plan sends rows to ends at {} ({name})", pct(after)));
    }
}

/// Bytes for people: 12 MB, 3.4 GB.
pub fn bytes(b: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = b.max(0.0);
    let mut k = 0;
    while v >= 1000.0 && k + 1 < units.len() {
        v /= 1000.0;
        k += 1;
    }
    if v >= 10.0 || k == 0 {
        format!("{v:.0} {}", units[k])
    } else {
        format!("{v:.1} {}", units[k])
    }
}

/// How long changing the cluster's shape from `from` to `to` (shards,
/// copies a shard) takes, and the rows it copies: what the operator does
/// for a `StrataCluster` patch, estimated once here so that every view says
/// the same (NEW-cluster-admin-7). New copies of every shard are sent their
/// shard's rows at once, [`COPY_ROWS_S`] each; slots move one run at a
/// time, at the rate the last moves here went (`measured_rows_s`), else
/// [`MOVE_ROWS_S`]; and each step takes [`STEP_S`] besides.
pub fn shape_estimate(snap: &Json, from: (usize, usize), to: (usize, usize), measured_rows_s: Option<f64>) -> Json {
    let shards = shards_of(snap);
    let rows: u64 = shards.iter().map(|s| s.docs).sum();
    let biggest = shards.iter().map(|s| s.docs).max().unwrap_or(0) as f64;
    let ((s0, r0), (s1, r1)) = (from, to);
    let move_rate = measured_rows_s.filter(|r| *r > 0.0).unwrap_or(MOVE_ROWS_S);
    let (mut copy, mut moved, mut secs, mut steps) = (0.0, 0.0, 0.0, 0usize);
    if r1 > r0 {
        // Every shard there now gains copies (a new shard's copies are
        // sent its rows by the moves); each primary sends them one by one.
        let d = (r1 - r0) as f64;
        copy = d * rows as f64;
        secs += d * biggest / COPY_ROWS_S;
        steps += 1;
    }
    if s1 != s0 {
        let d = s1.abs_diff(s0);
        moved = rows as f64 * d as f64 / s0.max(s1).max(1) as f64;
        secs += moved / move_rate;
        let slots = SLOTS * d / s0.max(s1).max(1);
        steps += d + slots.div_ceil(MOVE_SLOTS);
    }
    if r1 < r0 {
        steps += 1;
    }
    secs += steps as f64 * STEP_S;
    let total = (copy + moved).round();
    let rate = if measured_rows_s.is_some() { format!("slots move at {} rows a second, the rate the last moves here ran at", thousands(move_rate)) } else { format!("slots move at about {} rows a second (no move measured here yet)", thousands(move_rate)) };
    let text = if total > 0.0 {
        let mut parts = Vec::new();
        if copy > 0.0 {
            parts.push(format!("new copies are sent their shard's rows at about {} rows a second, every shard at once", thousands(COPY_ROWS_S)));
        }
        if moved > 0.0 {
            parts.push(rate.clone());
        }
        format!("About {} to copy {} rows: {}.", secs_words(secs), thousands(total), parts.join("; "))
    } else if steps > 0 {
        format!("About {}: no rows are copied.", secs_words(secs))
    } else {
        "Nothing changes.".into()
    };
    json!({
        "rows_to_copy": total as u64, "estimate_s": secs.round(), "copy_rows_s": COPY_ROWS_S, "move_rows_s": move_rate,
        "measured": measured_rows_s.is_some(), "text": text,
    })
}

/// The volume of a cluster's copies, as a `StrataCluster` would name it
/// ("252Gi"): the largest disk a copy reports in its health, for replaying
/// the volumes rule on machines of one's own (NEW-cluster-admin-6).
pub fn volume_of(cluster: &Json) -> Option<String> {
    let copies = cluster["shards"].as_array().into_iter().flatten().flat_map(|s| s["replicas"].as_array().into_iter().flatten());
    let most = copies.filter_map(|r| r["detail"]["load"]["disk_total_bytes"].as_f64()).fold(0.0, f64::max);
    let gib = (1u64 << 30) as f64;
    (most >= gib).then(|| format!("{}Gi", (most / gib).round()))
}

/// Seconds for people: "40 s", "6 min", "2 h 10 min".
fn secs_words(s: f64) -> String {
    let s = s.round().max(0.0) as u64;
    match s {
        0..=89 => format!("{s} s"),
        90..=3569 => format!("{} min", (s + 30) / 60),
        _ => {
            let m = (s + 30) / 60;
            if m % 60 == 0 { format!("{} h", m / 60) } else { format!("{} h {} min", m / 60, m % 60) }
        }
    }
}

/// A number for people: whole, with a comma every three digits.
pub fn thousands(n: f64) -> String {
    let digits = format!("{:.0}", n.abs());
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0.0 && digits != "0" {
        out.insert(0, '-');
    }
    out
}



impl Coordinator {
    /// Asks each server a plan adds (its `new_servers`) whether it can join:
    /// it answers, speaks the cluster's protocol, holds no rows and no
    /// cluster map, serves the cluster's first table, and has room for
    /// what it is sent. Each answer is a check of the plan's preflight.
    pub async fn probe_new(&self, p: &mut Json) {
        let list: Vec<(String, Option<u64>)> = p["new_servers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| Some((v["url"].as_str()?.to_string(), v["bytes"].as_u64())))
            // The command center names a server not given yet so.
            .filter(|(u, _)| !u.contains(".invalid") && u.contains("://"))
            .take(64)
            .collect();
        if list.is_empty() {
            return;
        }
        let asks = list.iter().map(|(u, b)| async move { (u.clone(), *b, tokio::time::timeout(Duration::from_secs(8), self.probe(u)).await) });
        let answers = futures::future::join_all(asks).await;
        let pre = p["preflight"].as_array_mut();
        let Some(pre) = pre else { return };
        let push = |pre: &mut Vec<Json>, level: &str, text: String| pre.push(json!({"ok": level != "fail", "level": level, "text": text}));
        for (u, bytes, r) in answers {
            let name = u.trim_start_matches("http://").trim_start_matches("https://").to_string();
            match r {
                Err(_) => push(pre, "fail", format!("{name} does not answer within 8 s: start it first")),
                Ok(Err(e)) => push(pre, "fail", e.message.replace(&u, &name)),
                Ok(Ok(d)) => {
                    let l = &d["load"];
                    let (total, free) = (l["disk_total_bytes"].as_f64(), l["disk_free_bytes"].as_f64());
                    let version = d["version"].as_str().map(|v| format!("version {v}, ")).unwrap_or_default();
                    match (total, free, bytes) {
                        (Some(t), Some(f), Some(b)) if t > 0.0 && b as f64 >= f * 0.98 => push(pre, "fail", format!("{name} has {} free, and is sent about {}: give it a bigger volume", self::bytes(f), self::bytes(b as f64))),
                        (Some(t), Some(f), b) if t > 0.0 => {
                            let after = (t - f + b.unwrap_or(0) as f64) / t;
                            let level = if after >= 0.85 { "warn" } else { "ok" };
                            push(pre, level, format!("{name} answers: {version}empty, its disk {:.0}% full{} ({} free)", (1.0 - f / t) * 100.0, b.map_or(String::new(), |_| format!(", {:.0}% once it has its rows", after * 100.0)), self::bytes(f)))
                        }
                        _ => push(pre, "ok", format!("{name} answers: {version}empty")),
                    }
                }
            }
        }
        let ok = pre.iter().all(|x| x["ok"] == true);
        p["ok"] = json!(ok);
    }

    /// What a server about to join says of itself ([`Self::admit`]'s checks
    /// first): its version and its last point (disk).
    async fn probe(&self, url: &str) -> Result<Json, ApiError> {
        let (mut c, _) = self.admit(url, false).await?;
        let h = c.health(crate::pb::Empty {}).await.map_err(|s| ApiError { status: 502, message: format!("{url}: {}", s.message()) })?.into_inner();
        let d: Json = serde_json::from_str(&h.detail).unwrap_or(Json::Null);
        Ok(json!({"version": d["system"]["version"], "load": d["load"]}))
    }
}

// ===================================================================== applying a plan

/// The plan this coordinator applies, or applied last ([`apply`]).
#[derive(Default)]
pub struct Runs {
    run: Mutex<Option<Run>>,
    /// Asked to stop after the step under way.
    stop: AtomicBool,
    /// Held while a plan runs.
    busy: Arc<tokio::sync::Mutex<()>>,
    /// Where the plan is kept between restarts (`None`: in memory only).
    file: OnceLock<Option<PathBuf>>,
}

/// A plan being applied, and how it went: kept in a file after each change.
#[derive(Clone, Serialize, Deserialize)]
struct Run {
    id: String,
    /// running, done, failed, stopped, interrupted (by a restart).
    state: String,
    started_ms: u64,
    #[serde(default)]
    finished_ms: Option<u64>,
    /// Who applied it (a key's name, "root token"), from where, and why.
    by: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    addr: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    /// The coordinator that runs it (its name or public URL).
    #[serde(default)]
    coordinator: Option<String>,
    /// The step under way, or the one it stopped at.
    at: usize,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    stopping: bool,
    steps: Vec<RunStep>,
    #[serde(default)]
    estimate_s: Option<f64>,
    /// This coordinator's p99 over the minute before it started.
    #[serde(default)]
    p99_before_ms: Option<f64>,
    #[serde(default)]
    interrupted_ms: Option<u64>,
    #[serde(default)]
    resumed: u32,
    #[serde(default)]
    resumed_by: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct RunStep {
    /// The step as asked, checked ([`step_of`]).
    step: Json,
    /// waiting, running, done, failed, stopped, interrupted.
    state: String,
    #[serde(default)]
    started_ms: Option<u64>,
    #[serde(default)]
    finished_ms: Option<u64>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    rows_done: u64,
    #[serde(default)]
    eta_s: Option<f64>,
    /// The servers of the shard it names (and of the shard slots go to)
    /// when the plan started: a step finds its shard by them, as a shard
    /// removed before it moves the shards after it down a place.
    #[serde(default)]
    servers: Option<Vec<String>>,
    #[serde(default)]
    to_servers: Option<Vec<String>>,
}

/// Who applies a plan, as their request says.
#[derive(Clone, Default)]
pub struct Who {
    pub by: String,
    pub key: Option<String>,
    pub client: Option<String>,
    pub addr: Option<String>,
    pub reason: Option<String>,
}

/// Why a step did not finish: stopped when asked, or failed.
enum Halt {
    Stopped(String),
    Failed(ApiError),
}

impl From<ApiError> for Halt {
    fn from(e: ApiError) -> Halt {
        Halt::Failed(e)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    crate::telemetry::now_ms()
}

/// The kinds of step a plan runs, under the names the command center and
/// [`plan`] give them.
fn kind_of(k: &str) -> Option<&'static str> {
    Some(match k {
        "add_copy" | "add" => "add_copy",
        "remove_copy" | "remove" => "remove_copy",
        "promote" | "handover" => "promote",
        "add_shard" | "shard" => "add_shard",
        "move" => "move",
        "rebalance" | "even" => "rebalance",
        "drain" => "drain",
        "remove_shard" | "unshard" => "remove_shard",
        _ => return None,
    })
}

/// Whether a kind of step runs a job of moves.
fn moves_kind(k: &str) -> bool {
    matches!(k, "move" | "rebalance" | "drain")
}

/// Step `k` as asked, checked, in one shape: its kind and what it names,
/// with the words, rows and estimate it was shown with.
fn step_of(k: usize, v: &Json) -> Result<Json, ApiError> {
    let bad = |what: &str| ApiError::bad(format!("step {}: {what}", k + 1));
    let kind = v["kind"].as_str().and_then(kind_of).ok_or_else(|| bad("its kind is add_copy, remove_copy, promote, add_shard, move, rebalance, drain or remove_shard"))?;
    let shard = || v["shard"].as_u64().ok_or_else(|| bad("it names its shard"));
    let url = || v["url"].as_str().or(v["copy"].as_str()).map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty()).ok_or_else(|| bad("it names its server's gRPC URL (url)"));
    let mut out = json!({"kind": kind, "text": v["text"].as_str().unwrap_or_default()});
    for f in ["rows", "est_s", "repair"] {
        if !v[f].is_null() {
            out[f] = v[f].clone();
        }
    }
    match kind {
        "add_copy" | "remove_copy" => {
            out["shard"] = json!(shard()?);
            out["url"] = json!(url()?);
        }
        "promote" => {
            out["shard"] = json!(shard()?);
            out["url"] = json!(url()?);
            out["force"] = json!(v["force"] == true);
        }
        "add_shard" => {
            let s = urls(&v["servers"], &format!("step {}: the new shard's servers", k + 1))?;
            if s.is_empty() {
                return Err(bad("a new shard needs a server at least"));
            }
            out["servers"] = json!(s);
        }
        "move" => {
            let mut runs = Vec::new();
            for r in v["slots"].as_array().into_iter().flatten() {
                match (r[0].as_u64(), r[1].as_u64()) {
                    (Some(a), Some(b)) if a < b && b as usize <= SLOTS => runs.push(json!([a, b])),
                    _ => return Err(bad("slots is a list of [start, end) runs of slots")),
                }
            }
            if runs.is_empty() {
                return Err(bad("it moves no slot"));
            }
            out["slots"] = json!(runs);
            out["to"] = json!(v["to"].as_u64().ok_or_else(|| bad("it names the shard the slots go to"))?);
            if let Some(f) = v["shard"].as_u64() {
                out["shard"] = json!(f);
            }
        }
        "drain" | "remove_shard" => out["shard"] = json!(shard()?),
        _ => {}
    }
    // A step sent without its words (a call by hand) is said from its fields.
    if out["text"] == "" {
        let i = out["shard"].as_u64().map_or(String::new(), |i| i.to_string());
        let u = out["url"].as_str().unwrap_or_default();
        out["text"] = json!(match kind {
            "add_copy" => format!("Add {u} to shard {i}"),
            "remove_copy" => format!("Remove {u} from shard {i}"),
            "promote" => format!("Make {u} the primary of shard {i}{}", if out["force"] == true { ", forced" } else { "" }),
            "add_shard" => format!("Add a shard served by {}", out["servers"].as_array().into_iter().flatten().filter_map(Json::as_str).collect::<Vec<_>>().join(", ")),
            "move" => format!("Move slots {} to shard {}", out["slots"].as_array().into_iter().flatten().map(|r| format!("{}..{}", r[0], r[1])).collect::<Vec<_>>().join(", "), out["to"]),
            "rebalance" => "Even the slots out over the shards".to_string(),
            "drain" => format!("Drain shard {i}: its slots move to the other shards"),
            _ => format!("Remove shard {i}"),
        });
    }
    Ok(out)
}

impl RunStep {
    fn kind(&self) -> &str {
        self.step["kind"].as_str().unwrap_or_default()
    }
}

impl Run {
    fn json(&self) -> Json {
        let now = now_ms();
        let steps: Vec<Json> = self
            .steps
            .iter()
            .map(|s| {
                let mut j = s.step.clone();
                j["state"] = json!(s.state);
                j["started_ms"] = json!(s.started_ms);
                j["finished_ms"] = json!(s.finished_ms);
                j["took_s"] = json!(s.started_ms.map(|t| (s.finished_ms.unwrap_or(now).saturating_sub(t)) as f64 / 1e3));
                j["note"] = json!(s.note);
                j["error"] = json!(s.error);
                j["rows_done"] = json!(s.rows_done);
                j
            })
            .collect();
        let done = self.steps.iter().filter(|s| s.state == "done").count();
        let rows = |s: &RunStep| s.step["rows"].as_f64().unwrap_or(0.0).max(0.0) as u64;
        let rows_total: u64 = self.steps.iter().map(rows).sum();
        let rows_done: u64 = self.steps.iter().map(|s| if s.state == "done" { rows(s).max(s.rows_done) } else { s.rows_done }).sum();
        // What is left: the step under way at its own pace (a job's ETA, or
        // what it was estimated less its time so far), and the estimates of
        // the steps after it.
        let eta_s = (self.state == "running").then(|| {
            let cur = self.steps.get(self.at);
            let here = cur.and_then(|s| s.eta_s.or_else(|| s.step["est_s"].as_f64().map(|e| (e - s.started_ms.map_or(0.0, |t| now.saturating_sub(t) as f64 / 1e3)).max(0.0))));
            let after: Option<f64> = self.steps.iter().skip(self.at + 1).map(|s| s.step["est_s"].as_f64()).sum();
            here.zip(after).map(|(a, b)| (a + b).round())
        });
        json!({
            "id": self.id, "state": self.state, "started_ms": self.started_ms, "finished_ms": self.finished_ms,
            "by": self.by, "client": self.client, "addr": self.addr, "reason": self.reason, "coordinator": self.coordinator,
            "at": self.at, "done": done, "of": self.steps.len(), "error": self.error, "stopping": self.stopping,
            "steps": steps, "rows_total": rows_total, "rows_done": rows_done.min(rows_total.max(rows_done)),
            "estimate_s": self.estimate_s, "eta_s": eta_s.flatten(), "p99_before_ms": self.p99_before_ms,
            "interrupted_ms": self.interrupted_ms, "resumed": self.resumed, "resumed_by": self.resumed_by,
        })
    }
}

/// This server's p99 over its last minute, in ms.
fn recent_p99(svc: &Service) -> Option<f64> {
    let points = svc.series.since(now_ms().saturating_sub(60_000));
    let w = crate::fleet::Window::of(&points);
    if w.hist.iter().sum::<u64>() == 0 {
        return None;
    }
    crate::telemetry::quantile(&crate::metrics::BUCKETS, &w.hist, 0.99).map(|s| (s * 1e4).round() / 10.0)
}

/// Where a coordinator keeps the plan it applies: `CHLOEDB_PLAN_STATE`,
/// else beside its audit log (`audit.log` keeps it in `audit.plan.json`).
fn state_file(svc: &Service) -> Option<PathBuf> {
    match chloedb::env::var("CHLOEDB_PLAN_STATE") {
        Ok(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => svc.audit.path().map(|p| p.with_extension("plan.json")),
    }
}

impl Runs {
    fn save(&self) {
        let Some(Some(path)) = self.file.get() else { return };
        let body = match lock(&self.run).as_ref() {
            Some(r) => serde_json::to_vec_pretty(r).unwrap_or_default(),
            None => {
                let _ = std::fs::remove_file(path);
                return;
            }
        };
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, path)) {
            tracing::warn!(error = %e, path = %path.display(), "could not keep the plan being applied");
        }
    }

    fn update(&self, f: impl FnOnce(&mut Run)) {
        if let Some(r) = lock(&self.run).as_mut() {
            f(r);
        }
        self.save();
    }

    fn step(&self, k: usize, f: impl FnOnce(&mut RunStep)) {
        if let Some(s) = lock(&self.run).as_mut().and_then(|r| r.steps.get_mut(k)) {
            f(s);
        }
    }
}

/// Takes up the plan a coordinator kept: one it was applying when it
/// stopped is `interrupted` at the step under way, an event says so, and
/// it waits to be resumed or discarded ([`resume`], [`discard`]).
pub fn start(svc: &Arc<Service>) {
    let Ok(c) = svc.coordinator() else { return };
    let file = state_file(svc);
    if c.plans.file.set(file.clone()).is_err() {
        return;
    }
    let Some(path) = file else { return };
    let Ok(bytes) = std::fs::read(&path) else { return };
    let mut run: Run = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "the plan kept here cannot be read: it is ignored");
            return;
        }
    };
    if run.state == "running" {
        run.state = "interrupted".into();
        run.stopping = false;
        run.interrupted_ms = Some(now_ms());
        let (at, n) = (run.at, run.steps.len());
        let text = run.steps.get(at).map(|s| s.step["text"].as_str().unwrap_or_default().to_string()).unwrap_or_default();
        if let Some(s) = run.steps.get_mut(at) {
            s.state = "interrupted".into();
            s.note = Some("This coordinator stopped while the step ran: it may be partly done. Resume runs it again from where it is.".into());
        }
        tracing::warn!(plan = %run.id, step = at + 1, of = n, "a plan was being applied when this coordinator stopped: resume or discard it");
        let title = format!("A plan stopped at step {} of {n} when the coordinator restarted{}", at + 1, if text.is_empty() { String::new() } else { format!(": {text}") });
        svc.events.add(crate::events::Event { kind: "change".into(), severity: "warning".into(), title, detail: json!({"plan": run.id, "state": "interrupted", "step": at + 1, "of": n, "route": "/v1/admin/plan/apply"}), where_: Json::Null, who: Some(run.by.clone()) });
    }
    *lock(&c.plans.run) = Some(run);
    c.plans.save();
}

/// The plan this coordinator applies or applied last, for `GET
/// /v1/admin/plan/run` and `/v1/cluster`: its steps and how each went,
/// rows, what is left, and this coordinator's p99 before and now.
pub fn view(svc: &Service) -> Json {
    let Ok(c) = svc.coordinator() else { return json!({"run": null}) };
    let mut run = lock(&c.plans.run).as_ref().map(Run::json);
    if let Some(r) = run.as_mut() {
        r["p99_now_ms"] = json!(recent_p99(svc));
    }
    let kept = c.plans.file.get().and_then(|f| f.as_ref()).map(|p| p.display().to_string());
    json!({"run": run, "kept_in": kept})
}

/// What another coordinator answered of its plan (`GET /v1/admin/plan/run`
/// asked as a peer), for the `elsewhere` list: its run, or why asking failed.
pub fn elsewhere(name: &str, url: &str, answer: Result<Json, String>) -> Json {
    match answer {
        Ok(j) => json!({"coordinator": name, "url": url, "run": j["run"]}),
        Err(e) => json!({"coordinator": name, "url": url, "run": null, "error": e}),
    }
}

/// Why a plan may not start here: another coordinator applies one now, or
/// one it applied stopped at a restart and waits to be resumed there.
pub fn busy_elsewhere(elsewhere: &Json) -> Option<String> {
    elsewhere.as_array()?.iter().find_map(|o| {
        let r = &o["run"];
        let at = || format!("step {} of {}", r["at"].as_u64().unwrap_or(0) + 1, r["of"].as_u64().unwrap_or(0));
        let who = o["coordinator"].as_str().unwrap_or("another coordinator");
        match r["state"].as_str()? {
            "running" => Some(format!("a plan is being applied on coordinator {who} ({}): wait for it, or stop it there", at())),
            "interrupted" => Some(format!("a plan stopped at {} when coordinator {who} restarted: resume it or discard it there first", at())),
            _ => None,
        }
    })
}

/// Why the move in the cluster map may not be finished here: another
/// coordinator runs a job of moves now (`elsewhere` of `GET
/// /v1/admin/moves`), and is making it.
pub fn moving_elsewhere(elsewhere: &Json) -> Option<String> {
    let o = elsewhere.as_array()?.first()?;
    let j = &o["job"];
    Some(format!(
        "coordinator {} moves slots now (move {} of {}): it finishes its own moves; stop it there if it must end",
        o["coordinator"].as_str().unwrap_or("?"),
        (j["done"].as_u64().unwrap_or(0) + 1).min(j["of"].as_u64().unwrap_or(1)),
        j["of"].as_u64().unwrap_or(1)
    ))
}

/// Starts applying a plan (`POST /v1/admin/plan/apply`): `body.steps`, in
/// order ([`step_of`]), run in the background; `body.estimate_s` is the
/// estimate it was shown with. Refused while a plan runs here, or one a
/// restart interrupted waits to be resumed or discarded.
pub fn apply(svc: &Arc<Service>, body: &Json, who: Who) -> Result<Json, ApiError> {
    let c = svc.coordinator()?.clone();
    let list = body["steps"].as_array().filter(|v| !v.is_empty()).ok_or_else(|| ApiError::bad("steps is the plan's list of steps, in order (POST /v1/admin/plan answers one)"))?;
    if list.len() > 10_000 {
        return Err(ApiError::bad("a plan runs 10,000 steps at most"));
    }
    let steps: Vec<Json> = list.iter().enumerate().map(|(k, v)| step_of(k, v)).collect::<Result<_, _>>()?;
    let Ok(busy) = c.plans.busy.clone().try_lock_owned() else {
        return Err(ApiError { status: 409, message: "a plan is being applied on this coordinator: wait for it, or stop it".into() });
    };
    if let Some(r) = lock(&c.plans.run).as_ref().filter(|r| r.state == "interrupted") {
        return Err(ApiError { status: 409, message: format!("a plan stopped at step {} of {} when this coordinator restarted: resume it or discard it first", r.at + 1, r.steps.len()) });
    }
    // The shards each step names, by their servers now and the copies the
    // plan adds them (a shard is found by those once its old copies left);
    // a new shard's are those its step adds it with.
    let t = c.topo();
    let n = t.replicas.len();
    let added: Vec<(u64, String)> = steps.iter().filter(|st| st["kind"] == "add_copy").filter_map(|st| Some((st["shard"].as_u64()?, st["url"].as_str()?.to_string()))).collect();
    let mut news: Vec<Vec<String>> = Vec::new();
    let mut run_steps = Vec::new();
    for (k, st) in steps.into_iter().enumerate() {
        let servers_of = |i: u64, news: &Vec<Vec<String>>| -> Option<Vec<String>> {
            let mut have: Vec<String> = match t.replicas.get(i as usize) {
                Some(rs) => rs.iter().map(|r| r.url.clone()).collect(),
                None => news.get(i as usize - n).cloned()?,
            };
            have.extend(added.iter().filter(|(j, u)| *j == i && !have.contains(u)).map(|(_, u)| u.clone()).collect::<Vec<_>>());
            Some(have)
        };
        let mut rs = RunStep { step: st.clone(), state: "waiting".into(), started_ms: None, finished_ms: None, note: None, error: None, rows_done: 0, eta_s: None, servers: None, to_servers: None };
        if let Some(i) = st["shard"].as_u64() {
            rs.servers = servers_of(i, &news);
            if rs.servers.is_none() && st["kind"] != "move" {
                return Err(ApiError::bad(format!("step {}: the cluster has no shard {i}, and no step before it adds one", k + 1)));
            }
        }
        if let Some(to) = st["to"].as_u64() {
            rs.to_servers = Some(servers_of(to, &news).ok_or_else(|| ApiError::bad(format!("step {}: the cluster has no shard {to}, and no step before it adds one", k + 1)))?);
        }
        if st["kind"] == "add_shard" {
            news.push(st["servers"].as_array().into_iter().flatten().filter_map(|u| u.as_str().map(String::from)).collect());
        }
        run_steps.push(rs);
    }
    let now = now_ms();
    let run = Run {
        id: format!("p{now}"),
        state: "running".into(),
        started_ms: now,
        finished_ms: None,
        by: who.by,
        key: who.key,
        client: who.client,
        addr: who.addr,
        reason: who.reason,
        coordinator: svc.identity.public_url.clone().or_else(|| svc.place.name.clone()),
        at: 0,
        error: None,
        stopping: false,
        steps: run_steps,
        estimate_s: body["estimate_s"].as_f64(),
        p99_before_ms: recent_p99(svc),
        interrupted_ms: None,
        resumed: 0,
        resumed_by: None,
    };
    *lock(&c.plans.run) = Some(run);
    c.plans.stop.store(false, Ordering::Relaxed);
    c.plans.save();
    tokio::spawn(run_from(svc.clone(), c, 0, busy, false));
    Ok(view(svc))
}

/// Asks the plan running here to stop once its step under way ends (a
/// job of moves stops after its move under way).
pub fn stop(svc: &Arc<Service>) -> Result<Json, ApiError> {
    let c = svc.coordinator()?;
    let kind = {
        let r = lock(&c.plans.run);
        let r = r.as_ref().filter(|r| r.state == "running").ok_or_else(|| ApiError { status: 409, message: "no plan is being applied on this coordinator".into() })?;
        r.steps.get(r.at).map(|s| s.kind().to_string()).unwrap_or_default()
    };
    c.plans.stop.store(true, Ordering::Relaxed);
    c.plans.update(|r| r.stopping = true);
    if moves_kind(&kind) {
        c.stop_moves();
    }
    Ok(view(svc))
}

/// Applies again a plan that stopped (a restart, a failed step, Stop) from
/// the step it stopped at: each step first looks at what is done already.
pub fn resume(svc: &Arc<Service>, by: String) -> Result<Json, ApiError> {
    let c = svc.coordinator()?.clone();
    let Ok(busy) = c.plans.busy.clone().try_lock_owned() else {
        return Err(ApiError { status: 409, message: "a plan is being applied on this coordinator already".into() });
    };
    let at = {
        let mut g = lock(&c.plans.run);
        let r = g.as_mut().ok_or_else(|| ApiError { status: 409, message: "no plan to resume".into() })?;
        if !matches!(r.state.as_str(), "interrupted" | "failed" | "stopped") {
            return Err(ApiError { status: 409, message: format!("the plan is {}: only one that stopped is resumed", r.state) });
        }
        r.state = "running".into();
        r.error = None;
        r.stopping = false;
        r.finished_ms = None;
        r.resumed += 1;
        r.resumed_by = Some(by);
        let at = r.at;
        if let Some(s) = r.steps.get_mut(at) {
            s.state = "waiting".into();
            s.error = None;
        }
        at
    };
    c.plans.stop.store(false, Ordering::Relaxed);
    c.plans.save();
    tokio::spawn(run_from(svc.clone(), c, at, busy, true));
    Ok(view(svc))
}

/// Forgets a plan that is not running (one that stopped stays until it is
/// resumed or discarded); answers the plan it was.
pub fn discard(svc: &Arc<Service>) -> Result<Json, ApiError> {
    let c = svc.coordinator()?;
    let Ok(_busy) = c.plans.busy.clone().try_lock_owned() else {
        return Err(ApiError { status: 409, message: "the plan is being applied: stop it first".into() });
    };
    let was = lock(&c.plans.run).take().map(|r| r.json()).ok_or_else(|| ApiError { status: 409, message: "no plan to discard".into() })?;
    c.plans.save();
    Ok(json!({"discarded": was}))
}

/// Runs the plan's steps from `from`, one after the other, stopping at the
/// first that fails or when asked; `resuming`: the first step looks at
/// what a run before it left (a move left in the cluster map is finished).
async fn run_from(svc: Arc<Service>, c: Arc<Coordinator>, from: usize, _busy: tokio::sync::OwnedMutexGuard<()>, resuming: bool) {
    let n = lock(&c.plans.run).as_ref().map_or(0, |r| r.steps.len());
    let mut outcome: (&str, Option<String>) = ("done", None);
    for k in from..n {
        if c.plans.stop.load(Ordering::Relaxed) {
            outcome = ("stopped", None);
            break;
        }
        c.plans.update(|r| {
            r.at = k;
            if let Some(s) = r.steps.get_mut(k) {
                s.state = "running".into();
                s.started_ms = Some(now_ms());
                s.finished_ms = None;
                s.note = None;
                s.error = None;
                s.eta_s = None;
            }
        });
        let t0 = Instant::now();
        let r = run_step(&svc, &c, k, resuming && k == from).await;
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let (state, status, note, error) = match &r {
            Ok(note) => ("done", 200, Some(note.clone()), None),
            Err(Halt::Stopped(note)) => ("stopped", 200, Some(note.clone()), None),
            Err(Halt::Failed(e)) => ("failed", e.status, None, Some(e.message.clone())),
        };
        c.plans.update(|r| {
            if let Some(s) = r.steps.get_mut(k) {
                s.state = state.into();
                s.finished_ms = Some(now_ms());
                s.eta_s = None;
                if note.is_some() {
                    s.note = note.clone().filter(|t| !t.is_empty());
                }
                s.error = error.clone();
            }
        });
        audit_step(&svc, &c, k, status, ms, error.as_deref());
        match r {
            Ok(_) => {}
            Err(Halt::Stopped(_)) => {
                outcome = ("stopped", None);
                break;
            }
            Err(Halt::Failed(e)) => {
                outcome = ("failed", Some(e.message));
                break;
            }
        }
    }
    let mut summary = None;
    c.plans.update(|r| {
        r.state = outcome.0.into();
        r.error = outcome.1.clone();
        r.stopping = false;
        r.finished_ms = Some(now_ms());
        summary = Some((r.id.clone(), r.at, r.steps.len(), r.by.clone(), r.reason.clone(), r.started_ms, r.steps.iter().filter(|s| s.state == "done").count()));
    });
    if let Some((id, at, n, by, reason, started, done)) = summary {
        let took = secs_words((now_ms().saturating_sub(started)) as f64 / 1e3);
        let (severity, title) = match outcome.0 {
            "done" => ("info", format!("Plan applied: {n} {} in {took}", if n == 1 { "step" } else { "steps" })),
            "stopped" => ("info", format!("Plan stopped when asked, after {done} of {n} steps")),
            _ => ("warning", format!("Plan stopped at step {} of {n}: {}", at + 1, outcome.1.clone().unwrap_or_default())),
        };
        tracing::info!(plan = %id, state = outcome.0, done, of = n, "plan ended");
        svc.events.add(crate::events::Event { kind: "change".into(), severity: severity.into(), title, detail: json!({"plan": id, "state": outcome.0, "done": done, "of": n, "route": "/v1/admin/plan/apply", "reason": reason}), where_: Json::Null, who: Some(by) });
    }
}

/// The audit entry of step `k`, done (status 200) or failed: as the admin
/// call it makes is audited, by the person who applied the plan, with the
/// plan's reason. Its event is the plan's (no change event of its own).
fn audit_step(svc: &Service, c: &Coordinator, k: usize, status: u16, ms: f64, error: Option<&str>) {
    let Some(run) = lock(&c.plans.run).clone() else { return };
    let Some(s) = run.steps.get(k) else { return };
    let st = &s.step;
    let i = st["shard"].as_u64();
    let url = st["url"].as_str().unwrap_or_default();
    let (method, route, what) = match s.kind() {
        "add_copy" => ("POST", "/v1/admin/shards/:i/replicas", format!("added {url} to shard {}", i.unwrap_or(0))),
        "remove_copy" => ("DELETE", "/v1/admin/shards/:i/replicas", format!("removed {url} from shard {}", i.unwrap_or(0))),
        "promote" => ("POST", "/v1/admin/shards/:i/promote", format!("made {url} the primary of shard {}{}", i.unwrap_or(0), if st["force"] == true { " (forced, asking no votes)" } else { "" })),
        "add_shard" => ("POST", "/v1/admin/shards", format!("added a shard served by {}", st["servers"].as_array().into_iter().flatten().filter_map(Json::as_str).collect::<Vec<_>>().join(", "))),
        "move" => ("POST", "/v1/admin/moves", format!("moved slots to shard {}", st["to"])),
        "rebalance" => ("POST", "/v1/admin/rebalance", "evened the slots out over the shards".to_string()),
        "drain" => ("POST", "/v1/admin/shards/:i/drain", format!("drained shard {}", i.unwrap_or(0))),
        _ => ("DELETE", "/v1/admin/shards/:i", format!("removed shard {}", i.unwrap_or(0))),
    };
    // A step that failed is said by what it tried: "add", not "added".
    let what = match error {
        Some(_) => [("added", "add"), ("removed", "remove"), ("made", "make"), ("moved", "move"), ("evened", "even"), ("drained", "drain")].iter().find_map(|(was, is)| what.strip_prefix(was).map(|rest| format!("{is}{rest}"))).unwrap_or(what),
        None => what,
    };
    let detail = format!("plan {}, step {} of {}: {what}{}", run.id, k + 1, run.steps.len(), error.map_or(String::new(), |e| format!(" (it failed: {e})")));
    svc.audit_keep(
        crate::keys::Entry {
            ts_ms: now_ms(),
            who: run.by.clone(),
            key: run.key.clone(),
            scope: Some("admin"),
            method: method.into(),
            route: route.into(),
            status,
            ms,
            request_id: Some(format!("{}-{}", run.id, k + 1)),
            detail: Some(detail),
            kind: crate::keys::CHANGE.into(),
            addr: run.addr.clone(),
            client: run.client.clone(),
            reason: run.reason.clone(),
            object: i.map(|i| format!("shard {i}")),
            before: None,
            after: Some(json!({"plan": run.id, "step": k + 1, "of": run.steps.len(), "kind": s.kind(), "error": error})),
            actor: "person".into(),
        },
        false,
    );
}

/// The shard a step names now: the one with any of the servers it had when
/// the plan started (`servers`), `i` itself first; else `i` when the step
/// recorded none.
fn find_shard(c: &Coordinator, servers: &Option<Vec<String>>, i: usize) -> Option<usize> {
    let t = c.topo();
    match servers {
        None => (i < t.replicas.len()).then_some(i),
        Some(s) => {
            let has = |rs: &Vec<Arc<super::Replica>>| rs.iter().any(|r| s.contains(&r.url));
            if t.replicas.get(i).is_some_and(has) {
                return Some(i);
            }
            t.replicas.iter().position(has)
        }
    }
}

/// Carries out step `k`: what it did, in a few words.
async fn run_step(svc: &Arc<Service>, c: &Arc<Coordinator>, k: usize, resuming: bool) -> Result<String, Halt> {
    let Some(s) = lock(&c.plans.run).as_ref().and_then(|r| r.steps.get(k).cloned()) else { return Ok(String::new()) };
    let st = &s.step;
    let i = st["shard"].as_u64().unwrap_or(0) as usize;
    let url = st["url"].as_str().unwrap_or_default().to_string();
    let gone = |what: &str| Halt::Failed(ApiError { status: 409, message: format!("shard {i} {what}: it is not in the cluster any more (another change removed it)") });
    let by = lock(&c.plans.run).as_ref().map(|r| r.by.clone()).unwrap_or_default();
    match s.kind() {
        "add_copy" => {
            let i = find_shard(c, &s.servers, i).ok_or_else(|| gone("was to get a copy"))?;
            let mut said = String::new();
            if c.topo().replicas[i].iter().any(|r| r.url == url) {
                said = "it was a copy of the shard already; ".into();
            } else {
                c.add_replica(i, &url, false).await?;
            }
            wait_sync(c, k, i, &url).await?;
            Ok(format!("{said}in sync with its primary"))
        }
        "remove_copy" => {
            let Some(i) = find_shard(c, &s.servers, i) else { return Ok("its shard was gone already".into()) };
            if !c.topo().replicas[i].iter().any(|r| r.url == url) {
                return Ok("it had left the shard already".into());
            }
            c.remove_replica(i, &url).await?;
            Ok(String::new())
        }
        "promote" => {
            let i = find_shard(c, &s.servers, i).ok_or_else(|| gone("was to get a new primary"))?;
            let snap = c.snapshot().await;
            let primary = snap["shards"].as_array().and_then(|v| v.get(i)).and_then(|s| s["primary"].as_str()).map(String::from);
            if primary.as_deref() == Some(url.as_str()) {
                return Ok("it was the primary already".into());
            }
            // A repair's promotion makes a copy that answers the primary in
            // place of a dead one: a primary that answers, elected meanwhile,
            // already is that (finding chaos-2).
            if let (true, Some(p)) = (st["repair"] == true, &primary) {
                return Ok(format!("{} was elected the primary meanwhile", c.copy_name(p)));
            }
            let j = c.promote(i, &url, st["force"] == true).await.map_err(|e| ApiError { message: plain_votes(&e.message), ..e })?;
            Ok(j["term"].as_u64().map_or(String::new(), |t| format!("term {t}")))
        }
        "add_shard" => {
            let servers: Vec<String> = st["servers"].as_array().into_iter().flatten().filter_map(|u| u.as_str().map(String::from)).collect();
            let t = c.topo();
            if let Some(j) = t.replicas.iter().position(|rs| rs.len() == servers.len() && rs.iter().all(|r| servers.contains(&r.url))) {
                return Ok(format!("it was shard {j} already"));
            }
            let j = c.add_shard(servers, crate::replication::Acks::All).await?;
            Ok(format!("it is shard {}", j["shard"]))
        }
        "move" => {
            let to = st["to"].as_u64().unwrap_or(0) as usize;
            let to = find_shard(c, &s.to_servers, to).ok_or_else(|| Halt::Failed(ApiError { status: 409, message: format!("the shard the slots go to (shard {to}) is not in the cluster any more") }))?;
            let map = c.map().ok_or_else(|| ApiError::bad("a coordinator routing by the hash before slots cannot move slots"))?;
            let slots: Vec<usize> = st["slots"].as_array().into_iter().flatten().filter_map(|r| Some((r[0].as_u64()? as usize)..(r[1].as_u64()? as usize))).flatten().filter(|&sl| sl < SLOTS && map.owner(sl) != to).collect();
            if slots.is_empty() {
                return Ok(format!("the slots were shard {to}'s already"));
            }
            wait_idle(c, k, resuming).await?;
            let count = slots.len();
            let j = c.start_move(slots, to)?;
            moves_event(svc, c, &by, format!("{count} slots move to shard {to}"), Some(to), &j);
            wait_job(c, k).await
        }
        "rebalance" => {
            wait_idle(c, k, resuming).await?;
            let j = c.rebalance(MOVE_SLOTS)?;
            if j["job"]["of"] == 0 {
                return Ok("the slots were even already".into());
            }
            moves_event(svc, c, &by, format!("Slots even out over the shards: {} moves", j["job"]["of"]), None, &j);
            wait_job(c, k).await
        }
        "drain" => {
            let i = find_shard(c, &s.servers, i).ok_or_else(|| gone("was to drain"))?;
            if c.map().is_some_and(|m| m.count(i) == 0) {
                return Ok("it held no slots already".into());
            }
            wait_idle(c, k, resuming).await?;
            let j = c.drain(i, MOVE_SLOTS)?;
            moves_event(svc, c, &by, format!("Shard {i} drains: {} moves", j["job"]["of"]), Some(i), &j);
            wait_job(c, k).await
        }
        _ => {
            let Some(i) = find_shard(c, &s.servers, i) else { return Ok("the shard was removed already".into()) };
            let j = c.remove_shard(i).await?;
            let servers: Vec<&str> = j["removed"].as_array().into_iter().flatten().filter_map(Json::as_str).collect();
            Ok(format!("its servers ({}) keep their databases until you stop them", servers.join(", ")))
        }
    }
}

/// A job of moves a step started: who started it, and its event.
fn moves_event(svc: &Service, c: &Coordinator, by: &str, title: String, shard: Option<usize>, j: &Json) {
    c.moves_by(by.to_string());
    let where_ = shard.map_or(Json::Null, |s| json!({"shard": s}));
    svc.events.add(crate::events::Event { kind: "moves_started".into(), severity: "info".into(), title, detail: json!({"kind": j["job"]["kind"], "of": j["job"]["of"], "plan": true}), where_, who: Some(by.to_string()) });
}

fn note(c: &Coordinator, k: usize, text: String) {
    c.plans.step(k, |s| s.note = Some(text));
}

/// Waits until no job of moves runs here and no move is under way in the
/// cluster map. A move left in the map by a coordinator that stopped is
/// finished when resuming (completed if it flipped, given up if not), and
/// otherwise fails the step after 30 s, saying so.
async fn wait_idle(c: &Arc<Coordinator>, k: usize, resuming: bool) -> Result<(), Halt> {
    let t0 = Instant::now();
    loop {
        let m = c.moves();
        let (local, moving) = (m["running"] == true, !m["moving"].is_null());
        if !local && !moving {
            return Ok(());
        }
        if c.plans.stop.load(Ordering::Relaxed) {
            return Err(Halt::Stopped("stopped before its moves started".into()));
        }
        if !local && moving && t0.elapsed() > Duration::from_secs(30) {
            if resuming {
                note(c, k, "finishing the move a coordinator left in the cluster map".into());
                c.finish_move().await?;
                continue;
            }
            return Err(Halt::Failed(ApiError { status: 409, message: "a move is left in the cluster map by a coordinator that stopped: finish it (Finish it, on Cluster), then resume the plan".into() }));
        }
        let mv = &m["moving"];
        let what = format!("slots {} from shard {} to shard {}, {}", mv["slots"].as_array().into_iter().flatten().map(|r| format!("{}..{}", r[0], r[1])).collect::<Vec<_>>().join(", "), mv["from"], mv["to"], mv["state"].as_str().unwrap_or("under way"));
        let left = 30u64.saturating_sub(t0.elapsed().as_secs());
        note(c, k, if local {
            "waiting for the job of moves under way to end".into()
        } else if resuming {
            format!("a coordinator that stopped left a move in the cluster map ({what}): this one finishes it in {left} s unless it ends first")
        } else {
            format!("waiting for the move in the cluster map to end ({what})")
        });
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Follows the job of moves step `k` started until it ends.
async fn wait_job(c: &Arc<Coordinator>, k: usize) -> Result<String, Halt> {
    loop {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let m = c.moves();
        let (done, of) = (m["done"].as_u64().unwrap_or(0), m["of"].as_u64().unwrap_or(0));
        let rows = m["rows_done"].as_u64().unwrap_or(0);
        let slots = m["job"]["slots_moved"].as_u64().unwrap_or(0);
        // CL-6: what is copied and what has flipped, said apart; the move
        // under way counts as under way, not done.
        let total = m["rows_total"].as_u64().filter(|t| *t > 0);
        let mut words = if of == 0 {
            "starting".to_string()
        } else {
            format!(
                "{} · {}{} rows copied · {} slots flipped",
                if done >= of { format!("{of} of {of} moves done") } else { format!("move {} of {of}", done + 1) },
                thousands(rows as f64),
                // The total is an estimate made at the start: past it, the
                // rows copied are said alone.
                total.filter(|t| rows <= *t).map_or(String::new(), |t| format!(" of about {}", thousands(t as f64))),
                thousands(slots as f64)
            )
        };
        if let Some(r) = m["rate_rows_s"].as_f64().filter(|r| *r > 0.0) {
            words += &format!(" · {} rows a second", thousands(r));
        }
        let eta = m["eta_s"].as_f64();
        c.plans.step(k, |s| {
            s.rows_done = rows;
            s.eta_s = eta;
            s.note = Some(words.clone());
        });
        match m["state"].as_str() {
            Some("done") if m["moving"].is_null() => return Ok(words),
            Some("failed") => return Err(Halt::Failed(ApiError { status: 500, message: m["job"]["error"].as_str().unwrap_or("a move failed").to_string() })),
            Some("stopped") => return Err(Halt::Stopped(format!("{words}; stopped after the move under way: the slots that moved stay where they are"))),
            _ => {}
        }
    }
}

/// Waits until `url`, a new copy of shard `i`, is in sync with the shard's
/// primary. Fails when it stops answering for a minute.
async fn wait_sync(c: &Arc<Coordinator>, k: usize, i: usize, url: &str) -> Result<(), Halt> {
    let mut down_since: Option<Instant> = None;
    loop {
        if c.plans.stop.load(Ordering::Relaxed) {
            return Err(Halt::Stopped("it goes on being sent the shard's rows; the plan stops here".into()));
        }
        let snap = c.snapshot().await;
        let shard = snap["shards"].as_array().and_then(|v| v.get(i)).cloned().unwrap_or(Json::Null);
        let copies = shard["replicas"].as_array().cloned().unwrap_or_default();
        let me = copies.iter().find(|r| r["url"] == url);
        let primary = copies.iter().find(|r| shard["primary"].as_str().is_some_and(|p| r["url"] == p));
        let f = primary.and_then(|p| p["detail"]["health"]["replication"]["replicas"].as_array()).and_then(|v| v.iter().find(|f| f["url"] == url)).cloned();
        match (me, &f) {
            (None, _) => return Err(Halt::Failed(ApiError { status: 409, message: format!("{url} is not a copy of shard {i} any more") })),
            (Some(r), _) if r["status"] == "down" => {
                let since = *down_since.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_secs(60) {
                    return Err(Halt::Failed(ApiError { status: 503, message: format!("{url} has not answered for a minute: {}", r["error"].as_str().unwrap_or("no answer")) }));
                }
                note(c, k, "it does not answer".into());
            }
            (Some(_), Some(f)) if f["in_sync"] == true && f["copying"] != true => return Ok(()),
            (Some(_), Some(f)) => {
                down_since = None;
                note(c, k, if f["copying"] == true { "being sent the shard's rows".into() } else { format!("{} transactions behind its primary", thousands(f["behind"].as_f64().unwrap_or(0.0))) });
            }
            (Some(_), None) => {
                down_since = None;
                note(c, k, "joining: its primary does not report it yet".into());
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// An error that quotes the votes a promotion asked for as JSON
/// (`[{"error": "...", "granted": false, "url": "..."}]`), in words.
pub fn plain_votes(msg: &str) -> String {
    let Some(at) = msg.find("[{") else { return msg.to_string() };
    let Ok(votes) = serde_json::from_str::<Vec<Json>>(&msg[at..]) else { return msg.to_string() };
    let said: Vec<String> = votes
        .iter()
        .map(|v| {
            let who = v["url"].as_str().unwrap_or("a copy").trim_start_matches("http://").trim_start_matches("https://").to_string();
            match (v["granted"].as_bool(), v["error"].as_str()) {
                (Some(true), _) => format!("{who} voted for it"),
                (_, Some(e)) => format!("{who} did not answer ({})", e.trim_start_matches("Unavailable: ")),
                _ => format!("{who} voted against it"),
            }
        })
        .collect();
    format!("{}{}", msg[..at].trim_end().trim_end_matches(':'), if said.is_empty() { String::new() } else { format!(": {}", said.join("; ")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_have_commas_and_advice_is_a_warning() {
        assert_eq!((thousands(7000.0), thousands(20_000.0), thousands(999.4), thousands(1_234_567.0)), ("7,000".into(), "20,000".into(), "999".into(), "1,234,567".into()));
    }

    fn snap() -> Json {
        let copy = |i: usize, k: usize, up: bool| json!({"url": format!("http://h{i}{k}"), "short": format!("s{i}-{k}"), "status": if up { "ok" } else { "down" }, "detail": {"health": {"docs": 6000}}});
        json!({"shards": [
            {"shard": 0, "primary": "http://h00", "failover": true, "slots": [[0, 2048]], "slots_held": 2048, "replicas": [copy(0, 0, true), copy(0, 1, true), copy(0, 2, true)]},
            {"shard": 1, "primary": "http://h10", "failover": true, "slots": [[2048, 4096]], "slots_held": 2048, "replicas": [copy(1, 0, true), copy(1, 1, true)]},
        ]})
    }

    #[test]
    fn a_move_another_coordinator_makes_is_not_finished_here() {
        assert_eq!(moving_elsewhere(&json!([])), None);
        let why = moving_elsewhere(&json!([{"coordinator": "coord-b", "url": "http://b:7700", "job": {"done": 2, "of": 6, "state": "running"}}])).unwrap();
        assert_eq!(why, "coordinator coord-b moves slots now (move 3 of 6): it finishes its own moves; stop it there if it must end");
    }

    #[test]
    fn a_plan_on_another_coordinator_holds_this_ones_back() {
        let running = elsewhere("coord-b", "http://b:7700", Ok(json!({"run": {"state": "running", "at": 1, "of": 3}})));
        assert_eq!(running["coordinator"], "coord-b");
        let failed = elsewhere("coord-c", "http://c:7700", Err("timed out".into()));
        assert_eq!((failed["run"].is_null(), failed["error"].as_str()), (true, Some("timed out")));
        assert_eq!(busy_elsewhere(&json!([failed.clone(), running])).as_deref(), Some("a plan is being applied on coordinator coord-b (step 2 of 3): wait for it, or stop it there"));
        let stopped = elsewhere("coord-b", "http://b:7700", Ok(json!({"run": {"state": "interrupted", "at": 0, "of": 2}})));
        assert!(busy_elsewhere(&json!([stopped])).unwrap().starts_with("a plan stopped at step 1 of 2 when coordinator coord-b restarted"));
        let done = elsewhere("coord-b", "http://b:7700", Ok(json!({"run": {"state": "done", "at": 2, "of": 2}})));
        assert_eq!(busy_elsewhere(&json!([done, failed])), None);
    }

    #[test]
    fn the_volume_is_the_largest_disk_a_copy_reports() {
        let mut sn = snap();
        assert_eq!(volume_of(&sn), None, "no copy says its disk");
        sn["shards"][0]["replicas"][1]["detail"]["load"] = json!({"disk_total_bytes": 100.0 * (1u64 << 30) as f64});
        sn["shards"][1]["replicas"][0]["detail"]["load"] = json!({"disk_total_bytes": 250.4 * (1u64 << 30) as f64});
        assert_eq!(volume_of(&sn).as_deref(), Some("250Gi"));
    }

    #[test]
    fn a_shape_is_estimated_once_from_the_measured_rate() {
        // Two shards of 6,000 rows: a copy more of each sends 12,000 rows,
        // every shard at once (6,000 at 20,000 a second, and a step).
        let e = shape_estimate(&snap(), (2, 3), (2, 4), None);
        assert_eq!(e["rows_to_copy"], 12000, "{e}");
        assert_eq!(e["estimate_s"], (6000.0 / COPY_ROWS_S + STEP_S).round(), "{e}");
        assert_eq!(e["text"], "About 5 s to copy 12,000 rows: new copies are sent their shard's rows at about 20,000 rows a second, every shard at once.");
        // A third shard takes a third of the rows, at the rate measured.
        let e = shape_estimate(&snap(), (2, 3), (3, 3), Some(1000.0));
        assert_eq!(e["rows_to_copy"], 4000, "{e}");
        assert_eq!(e["measured"], true);
        // 4 s of rows, the new shard and 6 runs of 256 slots, 5 s each.
        assert_eq!(e["estimate_s"], 4.0 + 7.0 * STEP_S, "{e}");
        assert!(e["text"].as_str().unwrap().ends_with("slots move at 1,000 rows a second, the rate the last moves here ran at."), "{e}");
        assert_eq!(shape_estimate(&snap(), (2, 3), (2, 3), None)["text"], "Nothing changes.");
        assert_eq!(shape_estimate(&snap(), (2, 3), (2, 2), None)["text"], "About 5 s: no rows are copied.");
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
    fn a_lone_copy_replaced_hands_over_to_the_new_one() {
        let mut sn = snap();
        sn["shards"][1]["primary"] = Json::Null;
        sn["shards"][1]["replicas"].as_array_mut().unwrap().truncate(1);
        let p = plan(&sn, &json!({"shards": [{"shard": 1, "copies": ["http://new"]}]}), false).unwrap();
        let kinds: Vec<&str> = p["steps"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["add_copy", "handover", "remove_copy"], "{p}");
        assert_eq!((p["steps"][1]["copy"].as_str(), p["steps"][1]["url"].as_str()), (Some("http://h10"), Some("http://new")), "{p}");
        // A primary with copies it keeps hands over to one of those.
        let p = plan(&snap(), &json!({"shards": [{"shard": 1, "copies": ["http://h11", "http://new"]}]}), false).unwrap();
        assert_eq!(p["steps"][1]["url"], "http://h11", "{p}");
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
        // Two copies left with failover is advice, not a failed check.
        let warn = p["preflight"].as_array().unwrap().iter().find(|c| c["text"].as_str().unwrap().contains("left with 2 copies")).cloned().unwrap();
        assert_eq!((warn["level"].as_str(), warn["ok"].as_bool()), (Some("warn"), Some(true)), "{p}");
        assert_eq!(p["ok"], true, "{p}");
        assert!(p["estimate_from"].as_str().unwrap().starts_with("About 20,000 rows a second to a new copy, 7,000 rows a second for moves"), "{p}");
    }

    /// A dead primary is still the primary to the coordinator, which
    /// removes it only once another copy is: a plan that replaces it
    /// makes the copy the others vote for the primary first, as a repair
    /// (skipped when they elect one meanwhile) (finding chaos-2).
    #[test]
    fn a_dead_primary_replaced_is_followed_by_the_newest_copy_first() {
        let mut snap = snap();
        let s0 = &mut snap["shards"][0];
        s0["primary"] = Json::Null;
        s0["replicas"][0] = json!({"url": "http://h00", "short": "s0-0", "status": "down", "role": "unknown", "last_role": "primary", "term": 3});
        s0["replicas"][1]["term"] = json!(3);
        s0["replicas"][1]["txn"] = json!(90);
        s0["replicas"][2]["term"] = json!(3);
        s0["replicas"][2]["txn"] = json!(95);
        let ask = json!({"shards": [{"shard": 0, "copies": ["http://h01", "http://h02", "http://new"]}]});
        let p = plan(&snap, &ask, false).unwrap();
        let kinds: Vec<&str> = p["steps"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["add_copy", "handover", "remove_copy"], "{p}");
        let h = &p["steps"][1];
        assert_eq!((h["url"].as_str(), h["repair"].as_bool()), (Some("http://h02"), Some(true)), "{p}");
        assert_eq!(h["text"], "Make s0-2 shard 0's primary: s0-0, its primary, does not answer");
        assert_eq!(p["ok"], true, "{p}");
        // A live primary hands over as before, not as a repair.
        let p = plan(&self::snap(), &json!({"shards": [{"shard": 1, "copies": ["http://h11", "http://new"]}]}), false).unwrap();
        assert_eq!(p["steps"][1]["repair"], false);
    }

    #[test]
    fn a_drain_evens_the_other_shards_out_too() {
        // Shard 1 holds 3,048 slots and shard 0 548: draining shard 2 leaves
        // each 2,048, so 1,000 of shard 1's slots move too (NEW-cluster-admin-11).
        let shard = |i: usize, held: u64, docs: u64| json!({"shard": i, "primary": format!("http://h{i}"), "failover": true, "slots_held": held, "slots": [], "replicas": [{"url": format!("http://h{i}"), "short": format!("s{i}-0"), "status": "ok", "detail": {"health": {"rows": docs}}}]});
        let sn = json!({"shards": [shard(0, 548, 548), shard(1, 3048, 3048), shard(2, 500, 500)]});
        let p = plan(&sn, &json!({"remove_shards": [2]}), false).unwrap();
        assert_eq!(p["after"], json!([{"shard": 0, "slots": 2048, "rows": 2048.0}, {"shard": 1, "slots": 2048, "rows": 2048.0}]), "{p}");
        assert_eq!(p["steps"][0]["rows"], 1500, "{p}");
        assert!(p["steps"][0]["text"].as_str().unwrap().ends_with("which even out among themselves too (about 1,000 rows more)"), "{p}");
    }

    #[test]
    fn rows_count_every_table_and_disks_are_checked() {
        // A copy's rows of every table (20,000) and not the first's (6,000).
        let mut sn = snap();
        for r in sn["shards"][0]["replicas"].as_array_mut().unwrap() {
            r["detail"]["health"]["rows"] = json!(20_000);
            r["detail"]["load"] = json!({"disk_total_bytes": 100e9, "disk_free_bytes": 2.9e9, "file_bytes": 10e9, "wal_bytes": 0});
        }
        for r in sn["shards"][1]["replicas"].as_array_mut().unwrap() {
            r["detail"]["load"] = json!({"disk_total_bytes": 100e9, "disk_free_bytes": 50e9, "file_bytes": 1e9, "wal_bytes": 0});
        }
        let p = plan(&sn, &json!({"remove_shards": [1]}), false).unwrap();
        assert_eq!(p["rows_to_copy"], 6000, "shard 1 has 6,000 rows (docs only, an older server)");
        let p = plan(&sn, &json!({"moves": [{"slots": [0, 1024], "to": 1}]}), false).unwrap();
        assert_eq!(p["rows_to_copy"], 10_000, "{p}");
        assert_eq!(p["steps"][0]["rows"], 10_000);
        // Shard 1's copies are sent about 5 GB of 50 GB free: they end at 55%.
        let d = p["disks"].as_array().unwrap();
        assert_eq!(d.len(), 2, "{p}");
        assert_eq!(d[0]["after"], 0.55, "{p}");
        let texts: Vec<&str> = p["preflight"].as_array().unwrap().iter().filter_map(|x| x["text"].as_str()).collect();
        // Shard 0's copies are 97% full: not a green check, though the plan sends them nothing.
        assert!(texts.iter().any(|t| *t == "the disks of s0-0, s0-1 and s0-2 are 97% full now (the plan sends them nothing)"), "{texts:?}");
        assert!(p["preflight"].as_array().unwrap().iter().any(|x| x["level"] == "warn" && x["text"].as_str().unwrap().contains("97% full now")), "{p}");
        assert!(texts.iter().any(|t| t.starts_with("the disks have room: the fullest copy the plan sends rows to ends at 55%")), "{texts:?}");
        // Moving to shard 0 does not fit: 10 GB more where 3 GB are free.
        let p = plan(&sn, &json!({"moves": [{"slots": [2048, 4096], "to": 0}]}), false).unwrap();
        assert_eq!(p["ok"], false, "{p}");
        assert!(p["preflight"].as_array().unwrap().iter().any(|x| x["ok"] == false && x["text"].as_str().unwrap().starts_with("s0-0's disk would be 98% full after the plan (97% now, about 1.0 GB more)")), "{p}");
    }

    #[test]
    fn a_new_shard_evened_out_is_sent_its_share() {
        let mut sn = snap();
        for sh in sn["shards"].as_array_mut().unwrap() {
            for r in sh["replicas"].as_array_mut().unwrap() {
                r["detail"]["load"] = json!({"disk_total_bytes": 100e9, "disk_free_bytes": 50e9, "file_bytes": 3e9, "wal_bytes": 0});
            }
        }
        let p = plan(&sn, &json!({"shards": [{"shard": 2, "copies": ["http://n1", "http://n2", "http://n3"]}], "add_shards": 1, "even": true}), false).unwrap();
        let kinds: Vec<&str> = p["steps"].as_array().unwrap().iter().filter_map(|s| s["kind"].as_str()).collect();
        assert_eq!(kinds, ["add_shard", "rebalance"], "{p}");
        // A third of the slots (1,365) and of the rows move, all to the new shard.
        let t = p["steps"][1]["text"].as_str().unwrap();
        assert!(t.starts_with("Even the slots out over 3 shards: about 1,365 slots"), "{t}");
        assert!(t.contains("rows move to the new shard,"), "{t}");
        assert_eq!(p["after"].as_array().unwrap().iter().map(|a| a["slots"].as_u64().unwrap()).collect::<Vec<_>>(), [1366, 1365, 1365]);
        // Each new server is sent its shard's bytes: a third of the files.
        let b = p["new_servers"][0]["bytes"].as_f64().unwrap();
        assert!((b - 2e9).abs() < 0.05e9, "{p}");
        assert_eq!(step_of(1, &p["steps"][1]).unwrap()["kind"], "rebalance");
    }

    #[test]
    fn a_new_shard_of_two_copies_is_a_warning_and_new_servers_are_named() {
        let p = plan(&snap(), &json!({"shards": [{"shard": 2, "copies": ["http://n1", "http://n2"]}], "add_shards": 1}), false).unwrap();
        let w = p["preflight"].as_array().unwrap().iter().find(|x| x["text"].as_str().unwrap().starts_with("new shard 2 has 2 copies")).cloned();
        assert_eq!(w.map(|w| w["level"].clone()), Some(json!("warn")), "{p}");
        assert_eq!(p["new_servers"], json!([{"url": "http://n1", "bytes": null}, {"url": "http://n2", "bytes": null}]));
        assert_eq!(p["steps"][0]["servers"], json!(["http://n1", "http://n2"]));
    }

    #[test]
    fn steps_are_checked_into_one_shape() {
        let s = step_of(0, &json!({"kind": "add", "shard": 1, "url": "http://x/", "text": "x joins", "rows": 5, "est_s": 2})).unwrap();
        assert_eq!(s, json!({"kind": "add_copy", "shard": 1, "url": "http://x", "text": "x joins", "rows": 5, "est_s": 2}));
        let s = step_of(1, &json!({"kind": "handover", "shard": 1, "copy": "http://a", "url": "http://b", "force": true})).unwrap();
        assert_eq!((s["kind"].as_str(), s["url"].as_str(), s["force"].as_bool()), (Some("promote"), Some("http://b"), Some(true)));
        let s = step_of(2, &json!({"kind": "move", "shard": 0, "slots": [[0, 10], [20, 30]], "to": 1})).unwrap();
        assert_eq!(s["slots"], json!([[0, 10], [20, 30]]));
        assert_eq!(step_of(3, &json!({"kind": "even"})).unwrap()["kind"], "rebalance");
        // A step sent without its words is said from its fields.
        assert_eq!(step_of(0, &json!({"kind": "move", "slots": [[0, 683]], "to": 1})).unwrap()["text"], "Move slots 0..683 to shard 1");
        assert_eq!(step_of(0, &json!({"kind": "promote", "shard": 2, "url": "http://h:1", "force": true})).unwrap()["text"], "Make http://h:1 the primary of shard 2, forced");
        assert_eq!(step_of(4, &json!({"kind": "unshard", "shard": 2})).unwrap()["kind"], "remove_shard");
        let e = step_of(5, &json!({"kind": "move", "slots": [[10, 5]], "to": 1})).unwrap_err();
        assert!(e.message.starts_with("step 6: slots"), "{}", e.message);
        assert!(step_of(0, &json!({"kind": "reboot"})).is_err());
        assert!(step_of(0, &json!({"kind": "remove", "shard": 0})).is_err(), "a url");
        // The plan's own steps run as they are.
        let p = plan(&snap(), &json!({"shards": [{"shard": 1, "copies": ["http://h11", "http://new"]}]}), false).unwrap();
        let kinds: Vec<String> = p["steps"].as_array().unwrap().iter().enumerate().map(|(k, s)| step_of(k, s).unwrap()["kind"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["add_copy", "promote", "remove_copy"]);
        assert_eq!(step_of(1, &p["steps"][1]).unwrap()["url"], "http://h11");
    }

    #[test]
    fn votes_quoted_as_json_are_said_in_words() {
        let m = r#"http://127.0.0.1:9241: 1 of 2 copies would vote for this replica to be the primary of term 2, and it needs 2: [{"error":"Unavailable: tcp connect error","granted":false,"url":"http://127.0.0.1:9239"}]"#;
        assert_eq!(plain_votes(m), "http://127.0.0.1:9241: 1 of 2 copies would vote for this replica to be the primary of term 2, and it needs 2: 127.0.0.1:9239 did not answer (tcp connect error)");
        assert_eq!(plain_votes("no votes here"), "no votes here");
        assert_eq!((bytes(950.0), bytes(12_300_000.0), bytes(3.4e9)), ("950 B".into(), "12 MB".into(), "3.4 GB".into()));
        assert_eq!((secs_words(40.0), secs_words(200.0), secs_words(4000.0)), ("40 s".into(), "3 min".into(), "1 h 7 min".into()));
        assert_eq!((secs_words(3600.0), secs_words(5400.0)), ("1 h".into(), "1 h 30 min".into()));
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
