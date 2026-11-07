//! The stream offsets of slots that arrived from another shard.
//!
//! A shard keeps one offset per stream source (`_sources`): every event
//! of the source up to it that its slots take is applied here, and none
//! after it. A coordinator sends each batch's offsets to every shard,
//! rows or not, so the shards' offsets move together. Slots that arrive
//! from another shard come with that shard's state of them, which can be
//! at another offset than this shard's own slots: a move that flips while
//! a batch is in flight has the batch committed here (for this shard's
//! own slots) but not there (for the moving ones, whose writes were held
//! and are sent here again), and a shard just added has seen no offset of
//! a source the moving slots hold events of.
//!
//! So a move carries the offsets of its slots on the shard they leave
//! ([`crate::slots::Move::offsets`]), and the shard they arrive at keeps
//! each as an *arrival*, even one its own offset equals (a request routed
//! by the map before the flip, which gives the slots no events, can still
//! come after it and move the shard's own past theirs): a row of `_sources`,
//! `_strata/arrived <epoch> <runs> <source>`, whose offset is the slots'
//! (with `_strata/arrivals <source>` while the source has any). An event
//! of the source that one of those slots takes is checked against the
//! arrival's offset rather than the shard's. A request routed by a map of
//! the arrival's epoch or later is routed by a map in which this shard
//! holds the slots, so it carries every operation of its events for them:
//! its offsets move the arrival's on, and once that meets the shard's own
//! the arrival is dropped. A stream resumes from the least of a shard's
//! offsets ([`folded`]). A shard that held no slot before they arrived
//! (a shard just added) also takes the offsets as its own.

use std::collections::{BTreeMap, HashMap};

use strata::db::{Db, Source};
use strata::storage::{WriteTxn, SOURCES};
use strata::value::Value;

use crate::slots::{self, SlotMap};
use crate::ApiError;

/// Where the names of arrivals start in `_sources`.
pub(crate) const ARRIVED: &str = "_strata/arrived ";

/// The name in `_sources` that says a source has arrivals here.
pub(crate) const GATE: &str = "_strata/arrivals ";

/// The offset an arrival keeps when the shard the slots left had none of
/// the source: none of its events was applied to them.
const NONE: i64 = i64::MIN;

/// Slots that arrived under the cluster map of `epoch`, and a source's
/// offset for them: every event up to it, none after, `None` for none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Arrival {
    pub epoch: u64,
    pub runs: Vec<(usize, usize)>,
    pub source: String,
    pub offset: Option<i64>,
}

impl Arrival {
    /// Its row's name in `_sources`.
    pub fn name(&self) -> String {
        let runs: Vec<String> = self.runs.iter().map(|(a, b)| format!("{a}-{b}")).collect();
        format!("{ARRIVED}{} {} {}", self.epoch, runs.join(","), self.source)
    }

    /// The arrival a row of `_sources` named `name` keeps, if it is one.
    pub fn parse(name: &str, offset: i64) -> Option<Arrival> {
        let rest = name.strip_prefix(ARRIVED)?;
        let (epoch, rest) = rest.split_once(' ')?;
        let (runs, source) = rest.split_once(' ')?;
        let runs = runs
            .split(',')
            .map(|r| {
                let (a, b) = r.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .collect::<Option<Vec<(usize, usize)>>>()?;
        Some(Arrival { epoch: epoch.parse().ok()?, runs, source: source.to_string(), offset: (offset != NONE).then_some(offset) })
    }

    pub fn has(&self, slot: usize) -> bool {
        self.runs.iter().any(|&(a, b)| (a..b).contains(&slot))
    }

    fn write(&self, txn: &mut WriteTxn<'_>, now: i64) -> Result<(), ApiError> {
        txn.set_source(&self.name(), self.offset.unwrap_or(NONE), now)?;
        Ok(())
    }

    fn delete(&self, txn: &mut WriteTxn<'_>) -> Result<(), ApiError> {
        txn.table(SOURCES)?.delete(&Value::Str(self.name()))?;
        Ok(())
    }
}

/// The arrivals `_sources` holds, of every source.
fn all(db: &Db) -> Result<Vec<Arrival>, ApiError> {
    Ok(db.sources()?.into_iter().filter_map(|s| Arrival::parse(&s.name, s.offset)).collect())
}

/// The arrival whose slots hold `slot`: the latest that does.
fn of_slot(arrivals: &[Arrival], slot: usize) -> Option<&Arrival> {
    arrivals.iter().filter(|a| a.has(slot)).max_by_key(|a| a.epoch)
}

/// The sources' offsets as a stream resumes from them: each source's
/// least over this shard's offset and its arrivals' (a source whose
/// arrival says its slots hold none of it is left out, as one this shard
/// has no offset of, so that a stream of it starts over), without the
/// rows the server keeps for itself but the marks of its state: the
/// cluster it follows, a hold of its writes, a promotion
/// ([`super::feed::own_mark`]).
pub(crate) fn folded(all: Vec<Source>, mark: &str) -> Vec<Source> {
    let mut least: HashMap<String, Option<i64>> = HashMap::new();
    for s in &all {
        if let Some(a) = Arrival::parse(&s.name, s.offset) {
            let e = least.entry(a.source).or_insert(Some(i64::MAX));
            *e = match (*e, a.offset) {
                (Some(x), Some(y)) => Some(x.min(y)),
                _ => None,
            };
        }
    }
    all.into_iter()
        .filter(|s| !s.name.starts_with(mark) || super::feed::own_mark(&s.name))
        .filter_map(|mut s| match least.get(&s.name) {
            None => Some(s),
            Some(None) => None,
            Some(Some(o)) => {
                s.offset = s.offset.min(*o);
                Some(s)
            }
        })
        .collect()
}

/// The arrivals of the sources one group of requests writes from, as its
/// transaction finds them: read from the database once per source (the
/// last commit, which the transaction starts from: the writer is held),
/// then as the group changes them.
pub(crate) struct Group<'a> {
    db: &'a Db,
    of: HashMap<String, Vec<Arrival>>,
}

impl<'a> Group<'a> {
    pub fn new(db: &'a Db) -> Group<'a> {
        Group { db, of: HashMap::new() }
    }

    /// Source `name`'s arrivals.
    pub fn of(&mut self, txn: &mut WriteTxn<'_>, name: &str) -> Result<&[Arrival], ApiError> {
        if !self.of.contains_key(name) {
            let some = txn.source_offset(&format!("{GATE}{name}"))?.is_some();
            let found = match some {
                true => all(self.db)?.into_iter().filter(|a| a.source == name).collect(),
                false => Vec::new(),
            };
            self.of.insert(name.to_string(), found);
        }
        Ok(&self.of[name])
    }

    /// The offset an event of source `name` that `slot` takes is checked
    /// against: its arrival's, or else the shard's own (`stored`). `None`:
    /// none of the source's events was applied there.
    pub fn threshold(&mut self, txn: &mut WriteTxn<'_>, name: &str, slot: usize, stored: Option<i64>) -> Result<Option<i64>, ApiError> {
        Ok(match of_slot(self.of(txn, name)?, slot) {
            Some(a) => a.offset,
            None => stored,
        })
    }

    /// After a request routed by the map of `epoch` (0: not by a
    /// coordinator) wrote from source `name`, whose offsets in it reached
    /// `seen` and whose shard offset is now `now_stored`: each arrival of
    /// an epoch it was routed under moves on to `seen`, and is dropped once
    /// it meets the shard's.
    pub fn after(&mut self, txn: &mut WriteTxn<'_>, name: &str, epoch: u64, seen: i64, now_stored: Option<i64>, now: i64) -> Result<(), ApiError> {
        if self.of(txn, name)?.is_empty() {
            return Ok(());
        }
        let Some(arrivals) = self.of.get_mut(name) else { return Ok(()) };
        let mut kept = Vec::with_capacity(arrivals.len());
        for mut a in std::mem::take(arrivals) {
            if epoch != 0 && epoch < a.epoch {
                kept.push(a);
                continue;
            }
            let moved = a.offset.map_or(seen, |o| o.max(seen));
            if Some(moved) == now_stored {
                a.delete(txn)?;
                continue;
            }
            if a.offset != Some(moved) {
                a.offset = Some(moved);
                a.write(txn, now)?;
            }
            kept.push(a);
        }
        if kept.is_empty() {
            txn.table(SOURCES)?.delete(&Value::Str(format!("{GATE}{name}")))?;
        }
        *arrivals = kept;
        Ok(())
    }
}

/// Writes, in `txn`, what this shard (`shard`) keeps of the offsets of
/// the slots that change hands here as it takes cluster map `new` in
/// place of `old`: the arrivals of slots that leave are dropped, and slots
/// that arrive with the offsets of the move that brought them
/// ([`crate::slots::Move::offsets`]) are given them as arrivals, which a
/// shard that held no slot also takes as its own offsets.
/// Slots that arrive without offsets (a move flipped by a coordinator
/// before them) are kept as before: under the shard's own. Returns
/// whether it wrote anything.
pub(crate) fn flip(db: &Db, txn: &mut WriteTxn<'_>, old: &SlotMap, new: &SlotMap, shard: usize, now: i64) -> Result<bool, ApiError> {
    let changed: Vec<usize> = (0..slots::SLOTS).filter(|&s| (old.owner(s) == shard) != (new.owner(s) == shard)).collect();
    if changed.is_empty() {
        return Ok(false);
    }
    let mut wrote = false;
    // Arrivals of slots that change hands end here: those that leave are
    // no longer this shard's, and those that come take a new one.
    let mut by_source: BTreeMap<String, Vec<Arrival>> = BTreeMap::new();
    for a in all(db)? {
        by_source.entry(a.source.clone()).or_default().push(a);
    }
    for (source, arrivals) in &by_source {
        let mut left = 0;
        for a in arrivals {
            let runs = slots::runs((0..slots::SLOTS).filter(|&s| a.has(s) && !changed.contains(&s)));
            if runs == a.runs {
                left += 1;
                continue;
            }
            a.delete(txn)?;
            wrote = true;
            if !runs.is_empty() {
                Arrival { runs, ..a.clone() }.write(txn, now)?;
                left += 1;
            }
        }
        if left == 0 {
            txn.table(SOURCES)?.delete(&Value::Str(format!("{GATE}{source}")))?;
        }
    }
    let Some(mv) = new.moving().filter(|m| m.to == shard && !m.offsets.is_empty()) else { return Ok(wrote) };
    let arrived: Vec<usize> = changed.iter().copied().filter(|&s| new.owner(s) == shard && mv.has(s)).collect();
    if arrived.is_empty() {
        return Ok(wrote);
    }
    let runs = slots::runs(arrived.into_iter());
    let held_none = old.count(shard) == 0;
    // Every source either shard has an offset of: one the slots' shard
    // has none of applied nothing of it to them.
    let mine: Vec<String> = db.sources()?.into_iter().map(|s| s.name).filter(|n| !n.starts_with(super::MARK)).collect();
    let mut names: Vec<&String> = mv.offsets.keys().chain(mine.iter()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let theirs = mv.offsets.get(name).copied();
        let ours = txn.source_offset(name)?;
        if held_none && theirs != ours {
            // The shard's offsets are the slots': it holds no others.
            match theirs {
                Some(o) => txn.set_source(name, o, now)?,
                None => {
                    txn.table(SOURCES)?.delete(&Value::Str(name.clone()))?;
                }
            }
        }
        // An arrival even at the shard's own offset: a request routed by
        // the map before this one, which gives the slots none of its
        // events, can still come after this flip and move the shard's
        // offset past events of theirs that the shard they left held, and
        // that come here again by the new map.
        Arrival { epoch: new.epoch, runs: runs.clone(), source: name.clone(), offset: theirs }.write(txn, now)?;
        txn.set_source(&format!("{GATE}{name}"), 1, now)?;
        wrote = true;
    }
    Ok(wrote)
}

/// Each source's offset for the slots of `slots` here, for a move of them
/// to carry: the least over the arrivals that hold some of them, and the
/// shard's own if some arrival holds none (a source that one of those
/// says nothing of was applied to them is left out, as is one the shard
/// has no offset of for slots no arrival holds).
pub(crate) fn of_slots(all: Vec<Source>, slots: &[usize], mark: &str) -> BTreeMap<String, i64> {
    let arrivals: Vec<Arrival> = all.iter().filter_map(|s| Arrival::parse(&s.name, s.offset)).collect();
    let own: HashMap<&str, i64> = all.iter().filter(|s| !s.name.starts_with(mark)).map(|s| (s.name.as_str(), s.offset)).collect();
    let mut names: Vec<&str> = own.keys().copied().chain(arrivals.iter().map(|a| a.source.as_str())).collect();
    names.sort_unstable();
    names.dedup();
    let mut out = BTreeMap::new();
    for name in names {
        let mine: Vec<&Arrival> = arrivals.iter().filter(|a| a.source == name).collect();
        let mut least = Some(i64::MAX);
        let mut uncovered = false;
        for &slot in slots {
            match mine.iter().filter(|a| a.has(slot)).max_by_key(|a| a.epoch) {
                Some(a) => least = least.zip(a.offset).map(|(x, y)| x.min(y)),
                None => uncovered = true,
            }
        }
        if uncovered {
            least = least.zip(own.get(name).copied()).map(|(x, y)| x.min(y));
        }
        if let Some(o) = least.filter(|&o| o != i64::MAX) {
            out.insert(name.to_string(), o);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arrival_is_named_by_its_epoch_slots_and_source() {
        let a = Arrival { epoch: 12, runs: vec![(0, 256), (512, 520)], source: "clicks/0 eu".into(), offset: Some(81234) };
        assert_eq!(a.name(), "_strata/arrived 12 0-256,512-520 clicks/0 eu");
        assert_eq!(Arrival::parse(&a.name(), 81234), Some(a.clone()));
        assert!(a.has(255) && a.has(519) && !a.has(256) && !a.has(520));
        let none = Arrival { offset: None, ..a };
        assert_eq!(Arrival::parse(&none.name(), NONE), Some(none));
        assert_eq!(Arrival::parse("_strata/slots {}", 1), None);
    }

    fn src(name: &str, offset: i64) -> Source {
        Source { name: name.into(), offset, updated: None }
    }

    #[test]
    fn a_stream_resumes_from_the_least_of_a_shards_offsets() {
        let a = Arrival { epoch: 3, runs: vec![(0, 10)], source: "s".into(), offset: Some(7) };
        let b = Arrival { epoch: 3, runs: vec![(0, 10)], source: "t".into(), offset: None };
        let all = vec![src("s", 9), src("t", 4), src("u", 2), src(&a.name(), 7), src(&b.name(), NONE), src("_strata/arrivals s", 1)];
        let got: Vec<(String, i64)> = folded(all, "_strata/").into_iter().map(|s| (s.name, s.offset)).collect();
        assert_eq!(got, vec![("s".to_string(), 7), ("u".to_string(), 2)]);
    }

    #[test]
    fn a_move_carries_the_least_offset_of_its_slots() {
        let a = Arrival { epoch: 3, runs: vec![(0, 10)], source: "s".into(), offset: Some(7) };
        let b = Arrival { epoch: 4, runs: vec![(0, 3)], source: "s".into(), offset: Some(11) };
        let c = Arrival { epoch: 3, runs: vec![(0, 10)], source: "v".into(), offset: Some(5) };
        let all = vec![src("s", 9), src("t", 4), src(&a.name(), 7), src(&b.name(), 11), src(&c.name(), 5)];
        let of = |slots: &[usize]| of_slots(all.clone(), slots, "_strata/").into_iter().collect::<Vec<_>>();
        let o = |v: &[(&str, i64)]| v.iter().map(|(n, x)| (n.to_string(), *x)).collect::<Vec<_>>();
        // Slots no arrival holds: the shard's own offsets.
        assert_eq!(of(&[20, 21]), o(&[("s", 9), ("t", 4)]));
        // Some held by one: the least.
        assert_eq!(of(&[5, 20]), o(&[("s", 7), ("t", 4)]));
        // All held by arrivals: theirs alone, the latest for each slot.
        assert_eq!(of(&[1]), o(&[("s", 11), ("t", 4), ("v", 5)]));
        assert_eq!(of(&[1, 5]), o(&[("s", 7), ("t", 4), ("v", 5)]));
    }
}
