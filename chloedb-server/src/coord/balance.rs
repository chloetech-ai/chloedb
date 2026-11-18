//! Moves that even out the shards' keyed load rather than their counts of
//! slots: `POST /v1/admin/rebalance {"by": "load"}`.
//!
//! The load of a slot is what the coordinators routed to it by key, a
//! second, over a window ([`super::heat`]): the reads and writes of its
//! keys. A shard's load is the sum over the slots it holds. The planner
//! takes slots from the busiest shard to the quietest, one at a time, the
//! busiest slot that fits the gap first (so that as few slots move as can
//! be), until the busiest shard takes under [`TARGET_SKEW`] times the mean.
//! It plans nothing while the busiest shard is under [`super::heat::HOT_SKEW`]
//! (the coordinator's own line for a hot shard), so that a cluster near
//! even is not churned.
//!
//! Three limits:
//! - a slot is moved only when its load fits the gap: under what the donor
//!   takes over the mean and what the taker takes under it. A slot busier
//!   than that would make its new shard the hot one (a hot slot: one key or
//!   one `shard_by` value), so it stays, and the plan names it; the other
//!   slots of its shard may still move away from it;
//! - no shard holds more than [`MAX_SLOTS_SHARE`] times an even share of
//!   the slots, so that one quiet shard is not given most of the rows;
//! - each move takes at most `chunk` slots, of one shard to one other.

use serde_json::{json, Value as Json};

use crate::slots::{runs, SlotMap, SLOTS};

/// The plan stops once the busiest shard takes under this many times the
/// mean of the shards' load.
pub const TARGET_SKEW: f64 = 1.1;
/// The most slots a shard may hold after the plan, as a multiple of an
/// even share.
pub const MAX_SLOTS_SHARE: f64 = 1.5;

/// A plan by load: the moves, and the load before and after.
#[derive(Debug, Default)]
pub struct LoadPlan {
    /// `(slots, from, to)`, as [`super::plan_even`] gives them.
    pub moves: Vec<(Vec<usize>, usize, usize)>,
    /// Each shard's load (operations a second) before and after the moves.
    pub before: Vec<f64>,
    pub after: Vec<f64>,
    /// The slot that could not be moved because it alone takes more than
    /// the gap: `(slot, shard, load)`.
    pub unsplit: Option<(usize, usize, f64)>,
    /// Why there are no moves, when there are none.
    pub why: Option<String>,
    /// Shards passed over as takers because their disks had no room.
    pub disk_full: Vec<usize>,
}

/// The busiest shard's load as a multiple of the mean (0 for none).
pub fn skew(load: &[f64]) -> f64 {
    let total: f64 = load.iter().sum();
    if total <= 0.0 || load.is_empty() {
        return 0.0;
    }
    load.iter().cloned().fold(0.0, f64::max) / (total / load.len() as f64)
}

/// The moves, of at most `chunk` slots each, that even out `load` (each
/// slot's operations a second) over the shards of `map`. Every shard counts
/// in the mean, one holding no slots too: it is where load goes first.
pub fn plan_by_load(map: &SlotMap, load: &[f64], chunk: usize) -> LoadPlan {
    plan_by_load_within(map, load, chunk, &[], &[])
}

/// [`plan_by_load`] within the shards' disks: `per_slot[i]`, the bytes a
/// slot of shard `i` holds, and `room[i]`, the bytes shard `i` may take
/// ([`super::plan::slot_costs`]); a shard is given no slot that does not
/// fit its room. Empty slices: no limit.
pub fn plan_by_load_within(map: &SlotMap, load: &[f64], chunk: usize, per_slot: &[f64], room: &[f64]) -> LoadPlan {
    let n = map.shards();
    let mut owner: Vec<usize> = (0..SLOTS).map(|s| map.owner(s)).collect();
    let w = |s: usize| load.get(s).copied().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0);
    let mut shard = vec![0.0f64; n];
    let mut count = vec![0usize; n];
    for s in 0..SLOTS {
        shard[owner[s]] += w(s);
        count[owner[s]] += 1;
    }
    let before = shard.clone();
    let mut plan = LoadPlan { before: before.clone(), ..Default::default() };
    if n < 2 {
        plan.after = before;
        plan.why = Some("a cluster of one shard has nowhere to move load".into());
        return plan;
    }
    let total: f64 = shard.iter().sum();
    if total <= 0.0 {
        plan.after = before;
        plan.why = Some("no keyed load was counted in the window".into());
        return plan;
    }
    let mean = total / n as f64;
    let first = skew(&shard);
    if first < super::heat::HOT_SKEW {
        plan.after = before;
        plan.why = Some(format!("the busiest shard takes {first:.2}× the mean, under {}×: even enough", super::heat::HOT_SKEW));
        return plan;
    }
    let cap = ((SLOTS as f64 / n as f64) * MAX_SLOTS_SHARE).ceil() as usize;
    let cost = |i: usize| per_slot.get(i).copied().filter(|b| b.is_finite() && *b > 0.0).unwrap_or(0.0);
    let mut room: Vec<f64> = (0..n).map(|i| room.get(i).copied().unwrap_or(f64::INFINITY)).collect();
    let mut full = std::collections::BTreeSet::new();
    // Each shard's slots, busiest first.
    let mut by_shard: Vec<Vec<usize>> = vec![Vec::new(); n];
    for s in 0..SLOTS {
        if w(s) > 0.0 {
            by_shard[owner[s]].push(s);
        }
    }
    for v in &mut by_shard {
        v.sort_by(|a, b| w(*b).total_cmp(&w(*a)).then(a.cmp(b)));
    }
    // Slots moved from one shard to another, in the order picked.
    let mut moved: Vec<((usize, usize), Vec<usize>)> = Vec::new();
    loop {
        let d = (0..n).max_by(|&a, &b| shard[a].total_cmp(&shard[b]).then(b.cmp(&a))).unwrap_or(0);
        if shard[d] < mean * TARGET_SKEW {
            break;
        }
        // The quietest shard with room for a slot of the busiest.
        let fits = |i: usize| room[i] >= cost(d);
        for i in (0..n).filter(|&i| i != d && count[i] < cap && !fits(i)) {
            full.insert(i);
        }
        let Some(r) = (0..n).filter(|&i| i != d && count[i] < cap && fits(i)).min_by(|&a, &b| shard[a].total_cmp(&shard[b]).then(a.cmp(&b))) else { break };
        let gap = (shard[d] - mean).min(mean - shard[r]);
        if gap <= 0.0 {
            break;
        }
        let Some(k) = by_shard[d].iter().position(|&s| w(s) <= gap) else {
            // Even the quietest slot it has with load is bigger than the gap.
            break;
        };
        let s = by_shard[d].remove(k);
        owner[s] = r;
        room[r] -= cost(d);
        shard[d] -= w(s);
        shard[r] += w(s);
        count[d] -= 1;
        count[r] += 1;
        let pos = by_shard[r].partition_point(|&x| w(x) > w(s) || (w(x) == w(s) && x < s));
        by_shard[r].insert(pos, s);
        match moved.iter_mut().find(|(k, _)| *k == (d, r)) {
            Some((_, v)) => v.push(s),
            None => moved.push(((d, r), vec![s])),
        }
    }
    // The busiest shard is still hot, because of a slot too busy for any
    // other shard to take.
    let d = (0..n).max_by(|&a, &b| shard[a].total_cmp(&shard[b])).unwrap_or(0);
    if let Some(&hot) = by_shard[d].first() {
        if shard[d] >= mean * TARGET_SKEW && w(hot) > (shard[d] - mean) {
            plan.unsplit = Some((hot, d, w(hot)));
        }
    }
    for ((from, to), mut slots) in moved {
        slots.sort_unstable();
        for c in slots.chunks(chunk.max(1)) {
            plan.moves.push((c.to_vec(), from, to));
        }
    }
    plan.disk_full = full.into_iter().collect();
    if plan.moves.is_empty() && !plan.disk_full.is_empty() && plan.unsplit.is_none() {
        plan.why = Some(format!("the quieter shards ({}) have no room on their disks for the busiest shard's slots: they would pass {:.0}% full", plan.disk_full.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", "), super::plan::MOVE_DISK_LIMIT * 100.0));
    } else if plan.moves.is_empty() {
        plan.why = Some(match plan.unsplit {
            Some((s, d, l)) => format!("shard {d} is hot because of slot {s} ({l:.1}/s), which no other shard can take without being hot in turn: it is one key or one shard_by value"),
            None => "no slot fits between the busiest shard and the others".into(),
        });
    }
    plan.after = shard;
    plan
}

/// The other coordinators' counts over `window`, each slot's operations a
/// second (`GET /v1/cluster/slots?window=`), since each counts only what it
/// routes; and whose counts are in them: `[{coordinator, url?, ok, error?}]`,
/// this coordinator first.
pub async fn others_load(svc: &crate::service::Service, window: &str) -> (Vec<Vec<f64>>, Json) {
    let mut others: Vec<Vec<f64>> = Vec::new();
    let mut counted = vec![json!({"coordinator": "this", "ok": true})];
    for (p, r) in crate::peers::get_all(svc, &format!("/v1/cluster/slots?window={window}")).await {
        match r.and_then(|j| peer_load(&j, window)) {
            Ok(v) => {
                others.push(v);
                counted.push(json!({"coordinator": p.name, "url": p.url, "ok": true}));
            }
            Err(e) => counted.push(json!({"coordinator": p.name, "url": p.url, "ok": false, "error": e})),
        }
    }
    (others, Json::Array(counted))
}

/// Each slot's operations a second from another coordinator's answer to
/// `GET /v1/cluster/slots?window=`, or why it cannot be counted.
pub fn peer_load(j: &Json, window: &str) -> Result<Vec<f64>, String> {
    if j["by_slot"] == false {
        return Err("it counts nothing by slot".into());
    }
    let secs = j["slots_secs"].as_f64().filter(|s| *s > 0.0);
    let (true, Some(secs)) = (j["slots_window"] == window, secs) else {
        return Err("it does not count slots by window (an older version): its load is left out".into());
    };
    let mut v = vec![0.0; SLOTS];
    for s in j["slots"].as_array().into_iter().flatten() {
        if let (Some(slot), Some(r), Some(w)) = (s[0].as_u64(), s[2].as_f64(), s[3].as_f64()) {
            if let Some(x) = v.get_mut(slot as usize) {
                *x = (r + w) / secs;
            }
        }
    }
    Ok(v)
}

/// Moves of slots as the API shows them: `[{from, to, count, slots}]`,
/// the slots as `[start, end)` runs.
pub fn moves_json(moves: &[(Vec<usize>, usize, usize)]) -> Vec<Json> {
    moves.iter().map(|(slots, from, to)| json!({
        "from": from, "to": to, "count": slots.len(),
        "slots": runs(slots.iter().copied()).into_iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
    })).collect()
}

impl LoadPlan {
    /// The plan as the API shows it.
    pub fn to_json(&self) -> Json {
        let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
        json!({
            "before": {"load_per_s": self.before.iter().map(|&x| r3(x)).collect::<Vec<_>>(), "skew": r3(skew(&self.before))},
            "after": {"load_per_s": self.after.iter().map(|&x| r3(x)).collect::<Vec<_>>(), "skew": r3(skew(&self.after))},
            "moves": moves_json(&self.moves),
            "slots_moved": self.moves.iter().map(|(s, ..)| s.len()).sum::<usize>(),
            "unsplit": self.unsplit.map(|(slot, shard, l)| json!({"slot": slot, "shard": shard, "load_per_s": r3(l)})),
            "disk_full": self.disk_full,
            "why": self.why,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(n: usize) -> SlotMap {
        let mut m = SlotMap::even(1, 1).with_replicas(vec![vec!["s0".into()]]).unwrap();
        for i in 1..n {
            m = m.add_shard(vec![format!("s{i}")]).unwrap();
        }
        // Even runs of slots.
        for (s, _, to) in super::super::plan_even(&m, None, SLOTS) {
            m = m.move_slots(&s, to).unwrap();
        }
        m
    }

    fn apply(m: &SlotMap, p: &LoadPlan) -> SlotMap {
        let mut m = m.clone();
        for (s, _, to) in &p.moves {
            m = m.move_slots(s, *to).unwrap();
        }
        m
    }

    fn shard_load(m: &SlotMap, load: &[f64]) -> Vec<f64> {
        let mut v = vec![0.0; m.shards()];
        for (s, l) in load.iter().enumerate() {
            v[m.owner(s)] += l;
        }
        v
    }

    #[test]
    fn a_hot_range_is_spread_over_the_others() {
        let m = map(3);
        // Every slot 1/s, and shard 0's slots 3/s more: 4× on shard 0.
        let load: Vec<f64> = (0..SLOTS).map(|s| if m.owner(s) == 0 { 4.0 } else { 1.0 }).collect();
        let p = plan_by_load(&m, &load, 256);
        assert!(skew(&p.before) > 1.9, "{:?}", p.before);
        assert!(!p.moves.is_empty());
        assert!(p.moves.iter().all(|(s, from, _)| *from == 0 && s.len() <= 256));
        let after = apply(&m, &p);
        let got = shard_load(&after, &load);
        assert!(skew(&got) < TARGET_SKEW + 0.01, "{got:?}");
        assert_eq!(got, p.after);
        assert!(p.unsplit.is_none());
        // No shard holds more than its cap of slots.
        let cap = ((SLOTS as f64 / 3.0) * MAX_SLOTS_SHARE).ceil() as usize;
        assert!((0..3).all(|i| after.count(i) <= cap));
        // Planned again, it is even: nothing to move.
        assert!(plan_by_load(&after, &load, 256).moves.is_empty());
    }

    #[test]
    fn a_hot_slot_stays_and_its_neighbours_move() {
        let m = map(2);
        let mut load = vec![1.0; SLOTS];
        // One slot of shard 0 takes as much as the rest of the cluster.
        let hot = (0..SLOTS).find(|&s| m.owner(s) == 0).unwrap();
        load[hot] = 4096.0;
        let p = plan_by_load(&m, &load, 4096);
        assert!(p.moves.iter().all(|(s, ..)| !s.contains(&hot)), "the hot slot does not move");
        assert_eq!(p.unsplit.map(|u| (u.0, u.1)), Some((hot, 0)));
        // Shard 0's other slots went to shard 1, as far as its cap lets them.
        assert!(!p.moves.is_empty());
        assert!(p.after[0] < p.before[0]);
        let j = p.to_json();
        assert_eq!(j["unsplit"]["slot"], hot);
    }

    #[test]
    fn a_single_hot_slot_alone_plans_nothing_and_says_why() {
        let m = map(2);
        let mut load = vec![0.0; SLOTS];
        let hot = (0..SLOTS).find(|&s| m.owner(s) == 1).unwrap();
        load[hot] = 100.0;
        let p = plan_by_load(&m, &load, 256);
        assert!(p.moves.is_empty());
        assert!(p.why.as_deref().unwrap().contains(&format!("slot {hot}")), "{:?}", p.why);
    }

    #[test]
    fn near_even_none_and_one_shard_plan_nothing() {
        let m = map(2);
        // 1.2× the mean on shard 0: under the line for a hot shard.
        let load: Vec<f64> = (0..SLOTS).map(|s| if m.owner(s) == 0 { 1.5 } else { 1.0 }).collect();
        let p = plan_by_load(&m, &load, 256);
        assert!(p.moves.is_empty() && p.why.as_deref().unwrap().contains("even enough"), "{:?}", p.why);
        assert!(plan_by_load(&m, &[0.0; SLOTS], 256).why.unwrap().contains("no keyed load"));
        assert!(plan_by_load(&map(1), &[1.0; SLOTS], 256).why.unwrap().contains("one shard"));
    }

    #[test]
    fn a_hot_range_of_uneven_slots_names_no_slot() {
        let m = map(3);
        // Shard 0's slots carry a long tail of load (1/rank), the others 0.2/s.
        let mut rank = 0.0;
        let load: Vec<f64> = (0..SLOTS).map(|s| if m.owner(s) == 0 { rank += 1.0; 400.0 / rank } else { 0.2 }).collect();
        let p = plan_by_load(&m, &load, 256);
        assert!(skew(&p.before) >= crate::coord::heat::HOT_SKEW, "{:?}", p.before);
        assert!(p.unsplit.is_none(), "an ordinary slot named: {:?}", p.unsplit);
        assert!(skew(&p.after) < TARGET_SKEW + 0.01, "{:?}", p.after);
    }

    #[test]
    fn another_coordinators_counts() {
        let j = json!({"by_slot": true, "slots_window": "15m", "slots_secs": 10.0, "slots": [[7, 0, 30, 10], [9, 1, 5, 0]]});
        let v = peer_load(&j, "15m").unwrap();
        assert_eq!((v[7], v[9], v[8]), (4.0, 0.5, 0.0));
        // Of a version before the windows: totals since it started, no window.
        let old = json!({"by_slot": true, "slots": [[7, 0, 30, 10]]});
        assert!(peer_load(&old, "15m").unwrap_err().contains("older version"));
        assert!(peer_load(&j, "5m").is_err(), "another window");
        assert!(peer_load(&json!({"by_slot": false}), "15m").unwrap_err().contains("nothing by slot"));
    }

    #[test]
    fn a_shard_without_room_on_its_disks_takes_nothing() {
        let m = map(3);
        let load: Vec<f64> = (0..SLOTS).map(|s| if m.owner(s) == 0 { 4.0 } else { 1.0 }).collect();
        // A slot is 1 MB; shard 1 has room for none, shard 2 for 50.
        let per_slot = vec![1e6; 3];
        let room = vec![f64::INFINITY, 0.0, 50.0 * 1e6];
        let p = plan_by_load_within(&m, &load, 4096, &per_slot, &room);
        assert!(p.moves.iter().all(|(_, _, to)| *to == 2), "{:?}", p.moves.iter().map(|m| (m.1, m.2)).collect::<Vec<_>>());
        assert!(p.moves.iter().map(|(s, ..)| s.len()).sum::<usize>() <= 50);
        // Shard 1 had no room; shard 2 none left once it took its 50.
        assert_eq!(p.disk_full, vec![1, 2]);
        assert_eq!(p.moves.iter().map(|(s, ..)| s.len()).sum::<usize>(), 50);
        // No room anywhere: nothing moves, and it says why.
        let none = plan_by_load_within(&m, &load, 4096, &per_slot, &[f64::INFINITY, 0.0, 0.0]);
        assert!(none.moves.is_empty());
        assert!(none.why.as_deref().unwrap().contains("no room on their disks"), "{:?}", none.why);
    }

    #[test]
    fn a_new_empty_shard_takes_load_first() {
        let two = map(2);
        let three = two.add_shard(vec!["s2".into()]).unwrap();
        let load = vec![1.0; SLOTS];
        let p = plan_by_load(&three, &load, 256);
        assert!(p.moves.iter().all(|(_, _, to)| *to == 2), "{:?}", p.moves.iter().map(|m| (m.1, m.2)).collect::<Vec<_>>());
        assert!(skew(&p.after) < TARGET_SKEW + 0.01, "{:?}", p.after);
    }
}
