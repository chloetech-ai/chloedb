//! The cluster simulator ([`chloedb_server::sim`]): seeded runs of a
//! shard's copies and its coordinators under faults keep the protocol's
//! promises, a seed replays exactly, and each bug the simulator found, put
//! back, is found again.
//!
//! `CHLOEDB_SIM_SEEDS=A..B` runs those seeds instead of the first few
//! hundred; `chloedb-sim --seed N --trace` shows what happened in one.

use std::ops::Range;

use chloedb_server::sim::{self, Bug, Config};

fn jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

fn seeds() -> Range<u64> {
    let Ok(s) = chloedb::env::var("CHLOEDB_SIM_SEEDS") else { return 0..400 };
    let (a, b) = s.split_once("..").expect("CHLOEDB_SIM_SEEDS is A..B");
    a.parse().expect("a seed")..b.parse().expect("a seed")
}

fn explain(failed: &[sim::Report]) -> String {
    let mut out = String::new();
    for r in failed {
        out += &format!("\nseed {} (chloedb-sim --seed {} --trace):", r.seed, r.seed);
        for v in &r.violations {
            out += &format!("\n  {v}");
        }
    }
    out
}

#[test]
fn seeded_runs_keep_the_protocols_promises() {
    let seeds = seeds();
    let failed = sim::run_seeds(seeds.clone(), &Config::of, jobs());
    assert!(failed.is_empty(), "{} of seeds {seeds:?} broke a promise:{}", failed.len(), explain(&failed));
}

#[test]
fn a_seed_replays_exactly() {
    for seed in 0..4 {
        let (a, b) = (sim::run(Config::of(seed), true), sim::run(Config::of(seed), true));
        assert_eq!(a.fingerprint, b.fingerprint, "seed {seed}");
        assert_eq!(a.events, b.events, "seed {seed}");
        assert_eq!(a.trace, b.trace, "seed {seed}");
        assert!(a.events > 1000 && a.stats.writes_ok > 0 && a.stats.reads_ok > 0, "seed {seed} did little: {:?}", a.stats);
    }
    assert_ne!(sim::run(Config::of(0), false).fingerprint, sim::run(Config::of(1), false).fingerprint);
}

/// The first seed below `within` that breaks a promise with `bug` put
/// back, looking a hundred seeds at a time.
fn finds(bug: Bug, within: u64) -> Option<sim::Report> {
    finds_with(bug, within, Config::of)
}

/// [`finds`], with each seed's configuration from `of`.
fn finds_with(bug: Bug, within: u64, of: impl Fn(u64) -> Config + Sync) -> Option<sim::Report> {
    let config = |s| {
        let mut c = of(s);
        c.bugs.push(bug);
        c
    };
    (0..within).step_by(100).find_map(|a| sim::run_seeds(a..(a + 100).min(within), &config, jobs()).into_iter().next())
}

// A promotion by hand that takes writes inside the replaced primary's
// lease (`Bug::TakesWritesInALease`) broke a read in one seed of 20,000,
// and in one of 5,000 at `--promote-ppm 100000` with pauses and slow links
// in every seed: too few for a test here, the unit tests of `cores::shard`
// check its fix.

#[test]
fn a_stalled_election_is_found_again() {
    // With elections that wait for no copy they do not need, and replicas
    // that wait after a vote, a minority votes for a candidate that loses
    // far less often: none of 12,000 seeds stalls with only this bug put
    // back. The server had both when the simulator found it.
    let of = |s| Config { bugs: vec![Bug::ReplacesTheCopyItElected], ..Config::of(s) };
    let r = finds_with(Bug::AsksAKnownTerm, 2000, of).expect("a seed that never elects a primary");
    assert!(r.violations.iter().any(|v| v.contains("0 primaries serve")), "{:?}", r.violations);
}

#[test]
fn a_replica_refusing_an_older_term_is_found_again() {
    let r = finds(Bug::RefusesOlderTerms, 3000).expect("a seed whose replicas never catch up");
    assert!(r.violations.iter().any(|v| v.contains("did not settle")), "{:?}", r.violations);
}

/// Calls dropped 15% of the time (`--drop-ppm 150000`), where a first
/// primary may stop before any replica heard from it.
fn lossy(s: u64) -> Config {
    Config { drop_ppm: 150_000, ..Config::of(s) }
}

#[test]
fn a_first_primary_lost_before_its_first_call_is_found_again() {
    let r = finds_with(Bug::WaitsForAFirstCall, 1000, lossy).expect("a seed whose first copies never elect a primary");
    assert!(r.violations.iter().any(|v| v.contains("0 primaries serve")), "{:?}", r.violations);
}

#[test]
fn a_shard_whose_calls_are_lost_elects_a_primary() {
    let failed = sim::run_seeds(0..400, &lossy, jobs());
    assert!(failed.is_empty(), "{} of 400 seeds at --drop-ppm 150000 broke a promise:{}", failed.len(), explain(&failed));
}

/// Seed `seed` with no drawn faults: its first primary (copy 0) is cut
/// off from every other node from 3 s for a minute, as the chaos monkey's
/// `primary-isolated` does.
fn primary_cut_off(seed: u64) -> Config {
    use std::time::Duration;
    let c = Config::of(seed);
    Config {
        copies: 3,
        crash_every: Duration::ZERO,
        cut_every: Duration::ZERO,
        pause_every: Duration::ZERO,
        slow_every: Duration::ZERO,
        drop_ppm: 0,
        promote_ppm: 0,
        faults_for: Duration::from_secs(66),
        isolate: vec![(0, Duration::from_secs(3), Duration::from_secs(60))],
        ..c
    }
}

/// The most terms a seed of `seeds` began while its primary was cut off,
/// and that seed, with `bugs` put back; each run keeps the protocol's
/// promises.
fn most_elections(seeds: Range<u64>, bugs: &[Bug]) -> (usize, u64) {
    let runs = std::sync::Mutex::new((0, 0));
    sim::on_seeds(seeds, jobs(), &|seed| {
        let r = sim::run(Config { bugs: bugs.to_vec(), ..primary_cut_off(seed) }, false);
        assert!(r.ok(), "seed {seed}: {:?}", r.violations);
        let cut = 3_000_000..63_000_000;
        let terms = r.stats.began_at.iter().filter(|t| cut.contains(*t)).count();
        let mut worst = runs.lock().unwrap();
        *worst = (*worst).max((terms, seed));
    });
    runs.into_inner().unwrap()
}

#[test]
fn a_primary_cut_off_is_replaced_once() {
    let (terms, seed) = most_elections(0..300, &[]);
    assert!(terms <= 1, "seed {seed} began {terms} terms while its primary was cut off");
}

#[test]
fn replicas_replacing_each_other_are_found_again() {
    let (terms, seed) = most_elections(0..300, &[Bug::ReplacesTheCopyItElected]);
    assert!(terms > 5, "seed {seed} began only {terms} terms while its primary was cut off");
}
