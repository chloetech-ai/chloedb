//! What a coordinator routes to each slot: the reads and writes it sends
//! by key, slot by slot, and the queries it sends to every shard.
//!
//! A key's slot ([`crate::slots::slot`]) is the smallest part of the key
//! space that moves between shards, so counting by slot says both which
//! shard is hot and whether moving slots would cool it: a shard hot over
//! many of its slots (a hot range) loses that load with the slots it
//! gives away, while one hot because of a single slot (a hot key, or a
//! `shard_by` value most rows share) takes the slot's load wherever the
//! slot goes. Counts are kept by slot, not by shard, and a slot's shard
//! is read from the map when they are shown, so they stay true across
//! moves.
//!
//! Counted, as the coordinator routes them: each operation of a write, at
//! the slot of the value that places its row (the key, or `shard_by`);
//! each key of a read by key; each value a query's filter pins the
//! placing field to. A query that asks every shard (a ranking, a filter
//! on other fields, UPDATE and DELETE) has no slot and is counted once as
//! fan-out, a load each shard takes alike. A cluster whose shards are
//! from before slots (protocol 4) routes by another hash and counts
//! nothing by slot.
//!
//! Rates are over the last 1, 5 and 15 minutes ([`WINDOWS`]), from
//! snapshots of the counts: every 10 s for the minute, every 60 s for the
//! longer windows. There is no timer: a snapshot is taken by the first
//! count or read in a period, before it counts, so the counts it holds
//! are those at the period's start, and the counts at a time are those of
//! the first snapshot taken at or after it (nothing was counted in
//! between, or a snapshot would have been taken). The counts take one
//! relaxed add each; the snapshots about 1.5 MB at most (23 of 64 KB).
//!
//! With `--slot-heat-keys`, one count in [`KEY_SAMPLE`] also samples its
//! key into a short list of the keys counted most (space-saving, the
//! [`HOT_KEYS`] most, halved every 5 minutes so that it follows what is
//! hot now), which says which key makes a slot hot. It holds key values,
//! so only an admin reads it (`GET /v1/admin/slots/keys`).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value as Json};

use crate::slots::SLOTS;

/// The windows rates are shown over: name, length in ms.
pub const WINDOWS: [(&str, u64); 3] = [("1m", 60_000), ("5m", 300_000), ("15m", 900_000)];

/// The snapshot rings: the fine one for windows up to a minute, the coarse
/// one for longer. Each keeps a snapshot more than its longest window
/// needs, the one from before the window's start.
const FINE_MS: u64 = 10_000;
const FINE_KEEP: usize = 7;
const COARSE_MS: u64 = 60_000;
const COARSE_KEEP: usize = 16;

/// Counts per slot: reads, then writes, then fan-out queries.
const READS: usize = 0;
const WRITES: usize = SLOTS;
const FANOUT: usize = 2 * SLOTS;
const COUNTS: usize = 2 * SLOTS + 1;

/// Below this many keyed operations a second, a window is `quiet`: too
/// little to call a shard hot.
pub const QUIET_PER_S: f64 = 1.0;
/// A shard is hot when it takes this many times the mean of the shards'
/// keyed load (the traffic report's balance check uses the same).
pub const HOT_SKEW: f64 = 1.3;
/// The hottest shard is hot because of one slot when that slot carries at
/// least this share of its keyed load.
pub const ONE_SLOT_SHARE: f64 = 0.5;

/// The keys sampled: one count in this many, and the most kept.
pub const KEY_SAMPLE: u64 = 16;
pub const HOT_KEYS: usize = 64;
/// The sampled keys' counts are halved every this many coarse periods.
const KEYS_HALVE_EVERY: u64 = 5;

/// How many slots of the top list a report shows.
pub const TOP_SLOTS: usize = 20;

struct Ring {
    period: u64,
    keep: usize,
    /// (period number, counts at its start), oldest first.
    snaps: VecDeque<(u64, Box<[u64]>)>,
}

impl Ring {
    fn new(period: u64, keep: usize) -> Ring {
        Ring { period, keep, snaps: VecDeque::new() }
    }

    /// Takes a snapshot when `now` is in a later period than the last.
    fn tick(&mut self, now: u64, counts: &dyn Fn() -> Box<[u64]>) -> bool {
        let p = now / self.period;
        if self.snaps.back().is_some_and(|(q, _)| *q >= p) {
            return false;
        }
        self.snaps.push_back((p, counts()));
        while self.snaps.len() > self.keep {
            self.snaps.pop_front();
        }
        true
    }

    /// The counts at the start of the window of `len` ms ending at `now`,
    /// and when it starts: the first period starting in it. `None` when
    /// nothing was counted since (the counts now are those then).
    fn at(&self, now: u64, len: u64) -> (u64, Option<&[u64]>) {
        let from = now.saturating_sub(len).div_ceil(self.period);
        let snap = self.snaps.iter().find(|(p, _)| *p >= from).map(|(_, c)| &c[..]);
        (from * self.period, snap)
    }
}

struct Rings {
    fine: Ring,
    coarse: Ring,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HotKey {
    pub table: String,
    pub key: String,
    pub slot: usize,
    pub reads: u64,
    pub writes: u64,
    /// How much of the count may be another key's (space-saving's error).
    pub err: u64,
}

/// The slot heat of a coordinator.
pub struct SlotHeat {
    counts: Box<[AtomicU64]>,
    /// When the fine ring takes its next snapshot (ms): checked by every
    /// count, so that only the first of a period takes the lock.
    next: AtomicU64,
    rings: Mutex<Rings>,
    keys: Option<Mutex<Vec<HotKey>>>,
    sampled: AtomicU64,
}

impl SlotHeat {
    /// `keys`: sample keys into the hot-key list (`--slot-heat-keys`).
    pub fn new(keys: bool) -> SlotHeat {
        SlotHeat {
            counts: (0..COUNTS).map(|_| AtomicU64::new(0)).collect(),
            next: AtomicU64::new(0),
            rings: Mutex::new(Rings { fine: Ring::new(FINE_MS, FINE_KEEP), coarse: Ring::new(COARSE_MS, COARSE_KEEP) }),
            keys: keys.then(|| Mutex::new(Vec::new())),
            sampled: AtomicU64::new(0),
        }
    }

    pub fn samples_keys(&self) -> bool {
        self.keys.is_some()
    }

    fn snapshot(&self) -> Box<[u64]> {
        self.counts.iter().map(|c| c.load(Ordering::Relaxed)).collect()
    }

    /// Takes the snapshots due at `now` (ms since the coordinator started).
    fn tick(&self, now: u64) {
        if now < self.next.load(Ordering::Relaxed) {
            return;
        }
        let mut r = self.rings.lock().unwrap_or_else(|e| e.into_inner());
        let take = || self.snapshot();
        r.fine.tick(now, &take);
        if r.coarse.tick(now, &take) && (now / COARSE_MS) % KEYS_HALVE_EVERY == 0 {
            if let Some(k) = &self.keys {
                let mut k = k.lock().unwrap_or_else(|e| e.into_inner());
                for e in k.iter_mut() {
                    e.reads /= 2;
                    e.writes /= 2;
                    e.err /= 2;
                }
                k.retain(|e| e.reads + e.writes > 0);
            }
        }
        // A caller with an older `now` that took the lock later must not
        // move it back.
        self.next.fetch_max((now / FINE_MS + 1) * FINE_MS, Ordering::Relaxed);
    }

    /// A read (`write` false) or write routed by a key in `slot`; `key`
    /// gives its table and the key's text when the key list samples it.
    pub fn count(&self, now: u64, slot: usize, write: bool, key: impl FnOnce() -> (String, String)) {
        self.tick(now);
        self.counts[if write { WRITES } else { READS } + slot].fetch_add(1, Ordering::Relaxed);
        if let Some(k) = &self.keys {
            if self.sampled.fetch_add(1, Ordering::Relaxed) % KEY_SAMPLE == 0 {
                let (table, key) = key();
                sample(&mut k.lock().unwrap_or_else(|e| e.into_inner()), table, key, slot, write);
            }
        }
    }

    /// A query sent to every shard.
    pub fn fan_out(&self, now: u64) {
        self.tick(now);
        self.counts[FANOUT].fetch_add(1, Ordering::Relaxed);
    }

    /// The counts since the coordinator started: per slot reads and
    /// writes, and fan-out queries.
    pub fn totals(&self) -> (Vec<u64>, Vec<u64>, u64) {
        let c = self.snapshot();
        (c[READS..WRITES].to_vec(), c[WRITES..FANOUT].to_vec(), c[FANOUT])
    }

    /// The counts over the window of `len` ms ending at `now`, and its
    /// length in seconds (shorter while the coordinator is younger).
    fn window(&self, now: u64, len: u64) -> (Vec<u64>, f64) {
        self.tick(now);
        let now_counts = self.snapshot();
        let r = self.rings.lock().unwrap_or_else(|e| e.into_inner());
        let ring = if len <= FINE_MS * (FINE_KEEP as u64 - 1) { &r.fine } else { &r.coarse };
        let (from, then) = ring.at(now, len);
        let delta = match then {
            Some(then) => now_counts.iter().zip(then).map(|(a, b)| a.saturating_sub(*b)).collect(),
            None => vec![0; COUNTS],
        };
        (delta, (now.saturating_sub(from)).max(1) as f64 / 1e3)
    }

    /// The report of `GET /v1/cluster/slots` at `now`: for each window the
    /// load of each shard (by `owner`, of `shards`), the slots most
    /// counted, how uneven the shards are and why. `all` adds every slot
    /// counted since the start.
    pub fn report(&self, now: u64, owner: &dyn Fn(usize) -> usize, shards: usize, all: bool) -> Json {
        let held: Vec<usize> = (0..shards).map(|i| (0..SLOTS).filter(|&s| owner(s) == i).count()).collect();
        let windows: serde_json::Map<String, Json> = WINDOWS.iter().map(|(name, len)| {
            let (d, secs) = self.window(now, *len);
            (name.to_string(), window_report(&d, secs, owner, &held))
        }).collect();
        let (reads, writes, fanout) = self.totals();
        let mut j = json!({
            "slot_count": SLOTS, "uptime_s": now as f64 / 1e3, "keys_sampled": self.samples_keys(),
            "totals": {"reads": reads.iter().sum::<u64>(), "writes": writes.iter().sum::<u64>(), "fanout": fanout},
            "windows": windows,
        });
        if all {
            j["slots"] = (0..SLOTS).filter(|&s| reads[s] + writes[s] > 0).map(|s| json!([s, owner(s), reads[s], writes[s]])).collect();
        }
        j
    }

    /// The short form in `/v1/cluster`: per shard, its keyed operations a
    /// second over the last 5 minutes and its share, and the verdict.
    pub fn brief(&self, now: u64, owner: &dyn Fn(usize) -> usize, shards: usize) -> (Vec<Json>, Json) {
        let held: Vec<usize> = (0..shards).map(|i| (0..SLOTS).filter(|&s| owner(s) == i).count()).collect();
        let (d, secs) = self.window(now, WINDOWS[1].1);
        let w = window_report(&d, secs, owner, &held);
        let per = w["shards"].as_array().cloned().unwrap_or_default().into_iter().map(|s| json!({"keyed_per_s": s["keyed_per_s"], "share": s["share"]})).collect();
        (per, json!({"window": WINDOWS[1].0, "verdict": w["verdict"], "skew": w["skew"]}))
    }

    /// The sampled keys, most counted first, with their counts estimated
    /// (sampled counts times [`KEY_SAMPLE`]).
    pub fn hot_keys(&self, now: u64, owner: &dyn Fn(usize) -> usize) -> Option<Json> {
        self.tick(now);
        let k = self.keys.as_ref()?.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut k = k;
        k.sort_by(|a, b| (b.reads + b.writes).cmp(&(a.reads + a.writes)));
        Some(json!({
            "sample": KEY_SAMPLE, "kept": HOT_KEYS, "halved_every_s": KEYS_HALVE_EVERY * COARSE_MS / 1000,
            "keys": k.iter().map(|e| json!({"table": e.table, "key": e.key, "slot": e.slot, "shard": owner(e.slot), "reads": e.reads * KEY_SAMPLE, "writes": e.writes * KEY_SAMPLE, "error": e.err * KEY_SAMPLE})).collect::<Vec<_>>(),
        }))
    }

    /// The figures `/metrics` shows: per shard and window, keyed reads and
    /// writes a second; fan-out queries a second; and the skew of the last
    /// 5 minutes.
    pub fn gauges(&self, now: u64, owner: &dyn Fn(usize) -> usize, shards: usize) -> (Vec<(String, f64)>, Vec<(String, f64)>, f64) {
        let held: Vec<usize> = (0..shards).map(|i| (0..SLOTS).filter(|&s| owner(s) == i).count()).collect();
        let (mut per_shard, mut fanout, mut skew) = (Vec::new(), Vec::new(), 0.0);
        for (name, len) in WINDOWS {
            let (d, secs) = self.window(now, len);
            let w = window_report(&d, secs, owner, &held);
            for s in w["shards"].as_array().into_iter().flatten() {
                let i = s["shard"].as_u64().unwrap_or(0);
                per_shard.push((format!("shard=\"{i}\",kind=\"read\",window=\"{name}\""), s["reads_per_s"].as_f64().unwrap_or(0.0)));
                per_shard.push((format!("shard=\"{i}\",kind=\"write\",window=\"{name}\""), s["writes_per_s"].as_f64().unwrap_or(0.0)));
            }
            fanout.push((format!("window=\"{name}\""), w["fanout_per_s"].as_f64().unwrap_or(0.0)));
            if name == WINDOWS[1].0 {
                skew = w["skew"].as_f64().unwrap_or(0.0);
            }
        }
        (per_shard, fanout, skew)
    }
}

/// Space-saving: the entry of `key` in `table` counted once more, or a new
/// one in place of the least counted, which takes its count as error.
fn sample(k: &mut Vec<HotKey>, table: String, key: String, slot: usize, write: bool) {
    let add = |e: &mut HotKey| if write { e.writes += 1 } else { e.reads += 1 };
    if let Some(e) = k.iter_mut().find(|e| e.key == key && e.table == table) {
        add(e);
        return;
    }
    if k.len() < HOT_KEYS {
        let mut e = HotKey { table, key, slot, reads: 0, writes: 0, err: 0 };
        add(&mut e);
        k.push(e);
        return;
    }
    let Some(min) = k.iter_mut().min_by_key(|e| e.reads + e.writes) else { return };
    let floor = min.reads + min.writes;
    *min = HotKey { table, key, slot, reads: 0, writes: 0, err: floor };
    if write {
        min.writes = floor + 1;
    } else {
        min.reads = floor + 1;
    }
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// One window of a report from its counts `d` over `secs`.
fn window_report(d: &[u64], secs: f64, owner: &dyn Fn(usize) -> usize, held: &[usize]) -> Json {
    let n = held.len();
    let mut reads = vec![0u64; n];
    let mut writes = vec![0u64; n];
    // Each shard's most counted slot, and its count.
    let mut top_of = vec![(0usize, 0u64); n];
    let mut counted: Vec<(usize, u64)> = Vec::new();
    for s in 0..SLOTS {
        let (r, w) = (d[READS + s], d[WRITES + s]);
        if r + w == 0 {
            continue;
        }
        let i = owner(s).min(n.saturating_sub(1));
        reads[i] += r;
        writes[i] += w;
        if r + w > top_of[i].1 {
            top_of[i] = (s, r + w);
        }
        counted.push((s, r + w));
    }
    counted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let keyed: u64 = reads.iter().sum::<u64>() + writes.iter().sum::<u64>();
    let load: Vec<u64> = (0..n).map(|i| reads[i] + writes[i]).collect();
    // The mean over the shards holding slots: one holding none takes no keyed load.
    let holding: Vec<usize> = (0..n).filter(|&i| held[i] > 0).collect();
    let mean = keyed as f64 / holding.len().max(1) as f64;
    let hot = holding.iter().copied().max_by_key(|&i| load[i]);
    let skew = match (hot, mean > 0.0) {
        (Some(i), true) => load[i] as f64 / mean,
        _ => 0.0,
    };
    let (verdict, why) = if keyed as f64 / secs < QUIET_PER_S {
        ("quiet", format!("fewer than {QUIET_PER_S} keyed operation a second: too few to say"))
    } else if holding.len() < 2 || skew < HOT_SKEW {
        ("even", format!("the busiest shard takes {:.2}× the mean of the keyed operations; queries sent to every shard (fan-out) are not in it, so a shard slow for those is not ruled out", skew.max(1.0)))
    } else {
        let i = hot.unwrap_or(0);
        let (slot, c) = top_of[i];
        let share = c as f64 / load[i].max(1) as f64;
        if share >= ONE_SLOT_SHARE {
            ("hot slot", format!("shard {i} takes {skew:.2}× the mean, {:.0}% of it in slot {slot}: a key or a shard_by value most of its load shares, which moving slots does not split", share * 100.0))
        } else {
            ("hot range", format!("shard {i} takes {skew:.2}× the mean, over many of its slots (its hottest, {slot}, {:.0}%): moving some of them to other shards evens it out", share * 100.0))
        }
    };
    json!({
        "secs": round(secs),
        "keyed_per_s": round(keyed as f64 / secs),
        "fanout_per_s": round(d[FANOUT] as f64 / secs),
        "shards": (0..n).map(|i| json!({
            "shard": i, "slots": held[i],
            "reads_per_s": round(reads[i] as f64 / secs), "writes_per_s": round(writes[i] as f64 / secs),
            "keyed_per_s": round(load[i] as f64 / secs),
            "share": if keyed == 0 { 0.0 } else { round(load[i] as f64 / keyed as f64) },
            "top_slot": (load[i] > 0).then_some(top_of[i].0),
            "top_slot_share": if load[i] == 0 { 0.0 } else { round(top_of[i].1 as f64 / load[i] as f64) },
        })).collect::<Vec<_>>(),
        "skew": round(skew),
        "top": counted.iter().take(TOP_SLOTS).map(|&(s, _)| json!({
            "slot": s, "shard": owner(s),
            "reads_per_s": round(d[READS + s] as f64 / secs), "writes_per_s": round(d[WRITES + s] as f64 / secs),
        })).collect::<Vec<_>>(),
        "verdict": verdict, "why": why,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halves(s: usize) -> usize {
        s * 2 / SLOTS
    }

    #[test]
    fn rates_cover_the_window_and_drop_what_is_older() {
        let h = SlotHeat::new(false);
        // 600 writes to slot 1 in the first minute, then 60 reads of slot 3000 in the next.
        for i in 0..600 {
            h.count(i * 100, 1, true, Default::default);
        }
        for i in 0..60 {
            h.count(60_000 + i * 1000, 3000, false, Default::default);
        }
        let r = h.report(120_000, &halves, 2, false);
        let m1 = &r["windows"]["1m"];
        assert_eq!(m1["secs"], 60.0);
        assert_eq!(m1["shards"][0]["writes_per_s"], 0.0, "{m1}");
        assert_eq!(m1["shards"][1]["reads_per_s"], 1.0, "{m1}");
        let m5 = &r["windows"]["5m"];
        // Younger than the window: the rates are over the 120 s there were.
        assert_eq!(m5["secs"], 120.0, "{m5}");
        assert_eq!(m5["shards"][0]["writes_per_s"], 5.0, "{m5}");
        assert_eq!(m5["shards"][1]["reads_per_s"], 0.5, "{m5}");
        assert_eq!(r["totals"]["writes"], 600);
        assert_eq!(r["totals"]["reads"], 60);
    }

    #[test]
    fn a_quiet_stretch_reads_as_none() {
        let h = SlotHeat::new(false);
        for i in 0..100 {
            h.count(i, 7, true, Default::default);
        }
        // Nothing counted for ten minutes: the last minute and five minutes saw none.
        let r = h.report(10 * 60_000, &halves, 2, false);
        assert_eq!(r["windows"]["1m"]["keyed_per_s"], 0.0);
        assert_eq!(r["windows"]["5m"]["keyed_per_s"], 0.0);
        assert_eq!(r["windows"]["1m"]["verdict"], "quiet");
        let m15 = &r["windows"]["15m"];
        assert!(m15["keyed_per_s"].as_f64().unwrap() > 0.0, "{m15}");
    }

    #[test]
    fn counts_follow_their_slot_to_its_new_shard() {
        let h = SlotHeat::new(false);
        for i in 0..1200 {
            h.count(i * 50, 10, false, Default::default);
        }
        let before = h.report(60_000, &halves, 2, false);
        assert_eq!(before["windows"]["1m"]["shards"][0]["share"], 1.0);
        // Slot 10 moved to shard 1.
        let moved = |s: usize| if s == 10 { 1 } else { halves(s) };
        let after = h.report(60_000, &moved, 2, false);
        assert_eq!(after["windows"]["1m"]["shards"][1]["share"], 1.0);
        assert_eq!(after["windows"]["1m"]["shards"][1]["slots"], 2049);
    }

    #[test]
    fn verdicts() {
        let at = |load: &dyn Fn(&SlotHeat)| {
            let h = SlotHeat::new(false);
            load(&h);
            h.report(59_999, &halves, 2, false)["windows"]["1m"].clone()
        };
        // Spread over both halves alike.
        let even = at(&|h| (0..6000).for_each(|i| h.count(i * 10, (i as usize * 7) % SLOTS, true, Default::default)));
        assert_eq!(even["verdict"], "even", "{even}");
        // One slot of shard 0 carries most of the load.
        let one = at(&|h| (0..6000).for_each(|i| h.count(i * 10, if i % 10 == 0 { 3000 } else { 5 }, false, Default::default)));
        assert_eq!(one["verdict"], "hot slot", "{one}");
        assert_eq!(one["shards"][0]["top_slot"], 5);
        assert_eq!(one["top"][0]["slot"], 5);
        // Shard 0's slots alike, shard 1's barely.
        let range = at(&|h| (0..6000).for_each(|i| h.count(i * 10, if i % 10 == 0 { 2048 + (i as usize % 2048) } else { i as usize % 2048 }, false, Default::default)));
        assert_eq!(range["verdict"], "hot range", "{range}");
        assert!(range["skew"].as_f64().unwrap() > HOT_SKEW);
        // Too little to tell.
        let quiet = at(&|h| (0..10).for_each(|i| h.count(i, 5, false, Default::default)));
        assert_eq!(quiet["verdict"], "quiet");
    }

    #[test]
    fn fan_out_is_counted_apart() {
        let h = SlotHeat::new(false);
        for i in 0..120 {
            h.fan_out(i * 500);
        }
        let w = &h.report(60_000, &halves, 2, false)["windows"]["1m"];
        assert_eq!(w["fanout_per_s"], 2.0, "{w}");
        assert_eq!(w["keyed_per_s"], 0.0);
    }

    #[test]
    fn hot_keys_keep_the_most_counted() {
        let h = SlotHeat::new(true);
        // Every count is sampled at a multiple of KEY_SAMPLE: a third of
        // those are "hot", the rest each a key of its own, many more than
        // the list keeps, so it evicts.
        for i in 0..(KEY_SAMPLE * 3000) {
            let n = i / KEY_SAMPLE;
            let k = if n % 3 == 0 { "hot".to_string() } else { format!("cold{n}") };
            h.count(i / 100, 9, n % 2 == 0, move || ("t".to_string(), k));
        }
        let k = h.hot_keys(30_000, &halves).unwrap();
        let keys = k["keys"].as_array().unwrap();
        assert_eq!(keys[0]["key"], "hot", "{k}");
        assert_eq!(keys.len(), HOT_KEYS);
        // The hot key's count is about a third of the samples, scaled.
        let hot = keys[0]["reads"].as_u64().unwrap() + keys[0]["writes"].as_u64().unwrap();
        assert!((900 * KEY_SAMPLE..=1100 * KEY_SAMPLE).contains(&hot), "{hot}");
        // An evicted key's successor carries its count as error.
        assert!(keys.iter().any(|e| e["error"].as_u64().unwrap() > 0), "{k}");
        assert!(SlotHeat::new(false).hot_keys(0, &halves).is_none());
    }

    #[test]
    fn sampled_keys_age() {
        let h = SlotHeat::new(true);
        for i in 0..(KEY_SAMPLE * 100) {
            h.count(i, 9, false, || ("t".to_string(), "k".to_string()));
        }
        let first = h.hot_keys(1_000, &halves).unwrap()["keys"][0]["reads"].as_u64().unwrap();
        // The next halving is at the next 5-minute mark.
        let later = h.hot_keys(5 * 60_000 + 1, &halves).unwrap()["keys"][0]["reads"].as_u64().unwrap();
        assert_eq!(later * 2, first);
    }
}
