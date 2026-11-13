//! The order a coordinator asks a shard's copies in for a read, by what it
//! observed of them (`--route least-cost`): a decision with no I/O, given
//! a view of each copy ([`Copy`]), the read's turn and a seed.
//!
//! The first copy asked is the rotation's (the turn's copy, as
//! `--route round-robin` asks), unless another, drawn at random, costs
//! less by more than [`MARGIN`] and by [`MIN_GAIN_MS`] at least: two
//! choices, anchored on the rotation.
//! Copies that cost about the same are asked in turn, as before, and only
//! one clearly worse is asked less. Coordinators keep no common view, so
//! each always choosing the copy that looks cheapest would send them all
//! to it at once; and one asked less is still asked, so what is known of
//! it stays current. The others follow by cost, for retries and hedges.
//!
//! A copy's cost ([`cost`]) is its recent query round-trip time, at least
//! [`FLOOR_MS`] (a copy in the same process or host answers in
//! microseconds, and the calls it holds must still count), times one more
//! than its calls in flight, raised for its recent errors, refusals for
//! its load (429) and the load it reported itself. A copy with no recent
//! round trip counts as the median of those that have one, neither
//! favoured nor avoided.
//!
//! What a copy's order was before stays: copies in their cool-off (failed
//! or outrun) are asked after the others, in turn; a copy the read must
//! ask first (`first`, the copy holding its lease or its kept statistics)
//! is first among those like it.

/// What the order is decided by, of one copy.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Copy {
    /// In its cool-off: asked after the others.
    pub avoided: bool,
    /// Its recent query round-trip time (ms), when recent enough.
    pub rtt_ms: Option<f64>,
    /// Calls to it from this coordinator not answered yet.
    pub inflight: u32,
    /// The share of recent calls that failed by its fault (0 to 1).
    pub errors: f64,
    /// The share of recent calls it refused for its load (0 to 1).
    pub refusals: f64,
    /// How busy it said it is (0 to 1: its query slots taken, or its query
    /// memory used, whichever is more), when it said so recently.
    pub busy: Option<f64>,
}

/// The least round-trip time a cost counts.
pub const FLOOR_MS: f64 = 1.0;
/// How much less another copy must cost than the rotation's to be asked
/// first instead: a quarter.
pub const MARGIN: f64 = 0.25;
/// How much less, at least, in cost (milliseconds, times the factors):
/// copies a few milliseconds away jitter by more than the margin from one
/// call to the next, and a copy passed over for such noise would not be
/// asked again until what is known of it is old.
pub const MIN_GAIN_MS: f64 = 1.0;
/// How much a copy's errors raise its cost: one call in ten failing
/// doubles it.
pub const ERROR_WEIGHT: f64 = 10.0;
/// How much its refusals raise its cost: one in four refused doubles it.
pub const REFUSAL_WEIGHT: f64 = 4.0;

/// The cost of asking `c`, `neutral_ms` standing for an unknown round trip.
pub fn cost(c: &Copy, neutral_ms: f64) -> f64 {
    let rtt = c.rtt_ms.unwrap_or(neutral_ms).max(FLOOR_MS);
    rtt * (1.0 + c.inflight as f64)
        * (1.0 + ERROR_WEIGHT * c.errors.clamp(0.0, 1.0))
        * (1.0 + REFUSAL_WEIGHT * c.refusals.clamp(0.0, 1.0))
        * (1.0 + c.busy.unwrap_or(0.0).clamp(0.0, 1.0))
}

/// The median of the known round trips, for the copies without one
/// ([`FLOOR_MS`] when none is known).
fn neutral(copies: &[Copy]) -> f64 {
    let mut known: Vec<f64> = copies.iter().filter_map(|c| c.rtt_ms).filter(|v| v.is_finite()).collect();
    if known.is_empty() {
        return FLOOR_MS;
    }
    known.sort_by(f64::total_cmp);
    let m = known.len() / 2;
    if known.len() % 2 == 1 { known[m] } else { (known[m - 1] + known[m]) / 2.0 }
}

/// A well-mixed 64-bit number from `x` (splitmix64).
pub fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The order to ask `copies` in, for the read whose turn is `start`, with
/// `first` asked first among those like it, and `seed` drawing the other
/// choice. The same arguments give the same order.
pub fn order(copies: &[Copy], start: usize, first: Option<usize>, seed: u64) -> Vec<usize> {
    let n = copies.len();
    if n == 0 {
        return Vec::new();
    }
    let rotation: Vec<usize> = (0..n).map(|k| (start + k) % n).collect();
    let neutral = neutral(copies);
    let costs: Vec<f64> = copies.iter().map(|c| cost(c, neutral)).collect();
    let open: Vec<usize> = rotation.iter().copied().filter(|&r| !copies[r].avoided).collect();
    let avoided: Vec<usize> = rotation.iter().copied().filter(|&r| copies[r].avoided).collect();
    let mut out = Vec::with_capacity(n);
    if !open.is_empty() {
        // The rotation's copy, or the other choice when it costs clearly less.
        let mut lead = open[0];
        if open.len() > 1 {
            let other = open[1 + (seed % (open.len() as u64 - 1)) as usize];
            if costs[other] < costs[lead] * (1.0 - MARGIN) && costs[lead] - costs[other] >= MIN_GAIN_MS {
                lead = other;
            }
        }
        out.push(lead);
        // The rest by cost, ties in turn (a stable sort of the rotation).
        let mut rest: Vec<usize> = open.iter().copied().filter(|&r| r != lead).collect();
        rest.sort_by(|&a, &b| costs[a].total_cmp(&costs[b]));
        out.extend(rest);
    }
    out.extend(avoided);
    // The copy the read must ask first, first among those like it.
    if let Some(f) = first.filter(|&f| f < n) {
        let at = out.iter().position(|&r| r == f).expect("every copy is ordered");
        let group_start = if copies[f].avoided { out.len() - out.iter().rev().take_while(|&&r| copies[r].avoided).count() } else { 0 };
        let r = out.remove(at);
        out.insert(group_start, r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(ms: f64) -> Copy {
        Copy { rtt_ms: Some(ms), ..Copy::default() }
    }

    /// How often each copy is asked first over `turns` reads.
    fn shares(copies: &[Copy], turns: usize) -> Vec<usize> {
        let mut firsts = vec![0; copies.len()];
        for t in 0..turns {
            firsts[order(copies, t, None, mix(t as u64))[0]] += 1;
        }
        firsts
    }

    #[test]
    fn equal_copies_are_asked_in_turn() {
        let copies = [known(5.0), known(5.0), known(5.0)];
        for start in 0..6 {
            assert_eq!(order(&copies, start, None, mix(start as u64)), vec![start % 3, (start + 1) % 3, (start + 2) % 3]);
        }
        // Nothing known of any: the same.
        assert_eq!(order(&[Copy::default(); 3], 4, None, 7), vec![1, 2, 0]);
    }

    #[test]
    fn copies_within_the_margin_are_asked_in_turn() {
        // 20% slower is within the margin: still half each.
        assert_eq!(shares(&[known(10.0), known(12.0)], 1000), vec![500, 500]);
    }

    #[test]
    fn copies_a_fraction_of_a_millisecond_apart_are_asked_in_turn() {
        // 35% apart, but by 0.7 ms: noise at this scale.
        assert_eq!(shares(&[known(2.0), known(2.7)], 1000), vec![500, 500]);
        // By more than a millisecond: the cheaper.
        assert_eq!(shares(&[known(2.0), known(3.5)], 1000), vec![1000, 0]);
    }

    #[test]
    fn a_clearly_slower_copy_is_asked_less_but_still_asked() {
        // Of two, the slow one is never the first choice: two choices of
        // two is the cheaper. It is still asked second, for retries.
        let two = [known(10.0), known(100.0)];
        assert_eq!(shares(&two, 1000), vec![1000, 0]);
        assert_eq!(order(&two, 1, None, 3), vec![0, 1]);
        // Of three, its turn goes to another when the draw finds a cheaper
        // one; it keeps its share only when it is the turn's copy and the
        // draw finds none cheaper, which here is never.
        let three = [known(10.0), known(10.0), known(100.0)];
        let s = shares(&three, 3000);
        assert_eq!(s[2], 0, "{s:?}");
        assert!(s[0] > 1200 && s[1] > 1200, "{s:?}");
    }

    #[test]
    fn calls_in_flight_count_even_on_a_copy_that_answers_at_once() {
        // In process: microseconds. Without a floor its in-flight calls
        // would not count, and it would take every read.
        let local = Copy { rtt_ms: Some(0.01), inflight: 8, ..Copy::default() };
        let remote = Copy { rtt_ms: Some(0.8), inflight: 0, ..Copy::default() };
        assert!(cost(&local, 1.0) > cost(&remote, 1.0));
        assert_eq!(shares(&[local, remote], 100), vec![0, 100]);
    }

    #[test]
    fn an_unknown_copy_counts_as_the_median_of_the_known() {
        let copies = [known(10.0), Copy::default(), known(30.0), known(20.0)];
        assert_eq!(neutral(&copies), 20.0);
        assert_eq!(cost(&copies[1], neutral(&copies)), cost(&known(20.0), 20.0));
        // A new copy beside fast ones is not flooded, nor shunned.
        let new = [known(10.0), Copy::default()];
        assert_eq!(shares(&new, 100), vec![50, 50]);
    }

    #[test]
    fn errors_refusals_and_reported_load_raise_the_cost() {
        let base = known(10.0);
        let c = cost(&base, 10.0);
        assert_eq!(cost(&Copy { errors: 0.1, ..base }, 10.0), 2.0 * c);
        assert_eq!(cost(&Copy { refusals: 0.25, ..base }, 10.0), 2.0 * c);
        assert_eq!(cost(&Copy { busy: Some(1.0), ..base }, 10.0), 2.0 * c);
        assert_eq!(cost(&Copy { busy: Some(7.0), ..base }, 10.0), 2.0 * c, "clamped");
    }

    #[test]
    fn copies_in_their_cool_off_come_last_in_turn() {
        let mut copies = [known(1.0), known(50.0), known(1.0), known(1.0)];
        copies[0].avoided = true;
        copies[2].avoided = true;
        let o = order(&copies, 0, None, 0);
        assert_eq!(&o[2..], &[0, 2], "{o:?}");
        assert_eq!(o[1], 1, "the slow open copy is asked after the fast one");
        // All in cool-off: in turn, as before.
        let all = [Copy { avoided: true, ..known(1.0) }; 3];
        assert_eq!(order(&all, 1, None, 9), vec![1, 2, 0]);
    }

    #[test]
    fn the_copy_a_read_must_ask_first_is_first_among_its_kind() {
        let copies = [known(1.0), known(1.0), known(100.0)];
        assert_eq!(order(&copies, 0, Some(2), 5)[0], 2);
        let mut avoided = copies;
        avoided[2].avoided = true;
        // In its cool-off: after the open copies, first of the others.
        let o = order(&avoided, 0, Some(2), 5);
        assert_eq!(o[2], 2, "{o:?}");
        avoided[1].avoided = true;
        let o = order(&avoided, 0, Some(2), 5);
        assert_eq!(o, vec![0, 2, 1]);
    }

    #[test]
    fn the_same_arguments_give_the_same_order() {
        let copies = [known(3.0), known(9.0), Copy::default(), known(40.0), Copy { inflight: 3, ..known(2.0) }];
        for t in 0..50 {
            let s = mix(t);
            assert_eq!(order(&copies, t as usize, None, s), order(&copies, t as usize, None, s));
            let mut o = order(&copies, t as usize, None, s);
            o.sort();
            assert_eq!(o, vec![0, 1, 2, 3, 4], "every copy once");
        }
    }
}
