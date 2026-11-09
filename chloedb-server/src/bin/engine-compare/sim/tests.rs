//! The planner's schedules against their profiles: rates, shares, Zipf's
//! law, sessions, typing, filters, writes, the warm-up and determinism.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::dist::zipf_head_share;
use super::fixture;
use super::gen::{self, eligible, Catalog, Plan};
use super::profile::Profile;
use super::schedule::{Entry, Op, Reader, Shape};
use crate::{prefix_of, Knobs};

const DOCS: usize = 5000;

fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| {
        let dir = fixture::write(&fixture::tempdir("catalog"), DOCS, 3000, 400);
        Catalog::load_dir("fixture", &dir).unwrap()
    })
}

fn profile(toml: &str) -> Profile {
    Profile::parse(toml).unwrap_or_else(|e| panic!("{e}\n{toml}"))
}

fn plan(p: &Profile, seed: u64, shape: Shape, name: &str) -> (Plan, Vec<Entry>, PathBuf) {
    let path = fixture::tempdir(name).join("schedule.jsonl.gz");
    let plan = gen::plan(p, "test", catalog(), seed, shape, &path).unwrap();
    (plan, read(&path), path)
}

fn read(path: &Path) -> Vec<Entry> {
    let mut r = Reader::open(path).unwrap();
    let mut out = Vec::new();
    while let Some(e) = r.next_entry() {
        out.push(e.unwrap());
    }
    assert!(r.stats.is_some(), "the trailer");
    out
}

fn timed(entries: &[Entry]) -> Vec<&Entry> {
    entries.iter().filter(|e| !e.warm).collect()
}

fn near(got: f64, want: f64, tol: f64, what: &str) {
    assert!((got - want).abs() <= tol, "{what}: {got}, expected {want} ± {tol}");
}

const FLAT: &str = r#"
name = "flat"
[mix]
bm25 = 1
vector = 1
"key lookup" = 1
[queries]
head = 500
zipf_s = 0.8
tail_share = 0.5
drift_every_s = 0
[traffic]
day_s = 600
warmup_s = 0
"#;

#[test]
fn a_flat_day_is_poisson_at_its_rate_with_its_mix() {
    let (fp, entries, _) = plan(&profile(FLAT), 1, Shape::Day { rate: 50.0 }, "flat");
    let s = &fp.stats;
    near(s.requests as f64, 30_000.0, 700.0, "requests");
    for k in ["bm25", "vector", "key lookup"] {
        near(s.by_kind[k] as f64 / s.requests as f64, 1.0 / 3.0, 0.02, k);
    }
    // Arrivals: exponential gaps, mean 1 / 50 s, coefficient of variation 1.
    let t: Vec<f64> = timed(&entries).iter().map(|e| e.t).collect();
    assert!(t.windows(2).all(|w| w[0] <= w[1]), "in time order");
    let gaps: Vec<f64> = t.windows(2).map(|w| w[1] - w[0]).collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    let sd = (gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / gaps.len() as f64).sqrt();
    near(mean, 0.02, 0.0006, "mean gap");
    near(sd / mean, 1.0, 0.03, "gaps' coefficient of variation");
    // Queries: half the draws new, the head by Zipf's law.
    near(s.tail_draws as f64 / s.query_draws as f64, 0.5, 0.02, "tail share");
    near(s.top_query_share, 0.5 * zipf_head_share(500, 0.8, 1), 0.006, "top query's share");
    // 10,000 tail draws, 2,200 queries past the head and 2,700 a pass after that: the tail starts over, and says so.
    assert_eq!(s.tail_wrapped, 3);
    near(s.first_time_share, s.distinct_queries as f64 / s.query_draws as f64, 1e-12, "first-time share");
    let (small, _, _) = plan(&profile(FLAT), 1, Shape::Day { rate: 2.0 }, "flat-small");
    assert_eq!(small.stats.tail_wrapped, 0);
    assert!(small.stats.first_time_share > 0.5, "first-time share {}", small.stats.first_time_share);
    let qs: BTreeSet<u64> = entries.iter().filter_map(|e| e.q).collect();
    assert_eq!(qs.len() as u64, s.distinct_queries);
    // Key lookups' keys are the collection's, row and key agreeing.
    for e in entries.iter().filter(|e| e.k == "key lookup").take(200) {
        assert_eq!(e.key.as_deref(), Some(format!("d{}", e.row.unwrap()).as_str()));
    }
}

#[test]
fn one_seed_one_file() {
    let p = profile(FLAT);
    let (a, _, pa) = plan(&p, 7, Shape::Day { rate: 20.0 }, "seed-a");
    let (b, _, pb) = plan(&p, 7, Shape::Day { rate: 20.0 }, "seed-b");
    let (c, _, _) = plan(&p, 8, Shape::Day { rate: 20.0 }, "seed-c");
    assert_eq!(std::fs::read(&pa).unwrap(), std::fs::read(&pb).unwrap(), "the same seed, the same bytes");
    assert_eq!(a.sha256, b.sha256);
    assert_ne!(a.sha256, c.sha256);
    // The hash is the uncompressed lines' (zcat | sha256sum).
    let mut r = Reader::open(&pa).unwrap();
    while r.next_entry().is_some() {}
    assert_eq!(r.hash(), a.sha256);
}

#[test]
fn the_day_bursts_and_flash_crowds_shape_the_rate() {
    let p = profile(&format!(
        "{}\npeak_to_trough = 3.0\nhumps = 1\npeak_at = 0.5\n",
        FLAT.replace("day_s = 600", "day_s = 1200")
    ));
    let (dp, entries, _) = plan(&p, 2, Shape::Day { rate: 60.0 }, "day");
    near(dp.stats.mean_rate, 40.0, 1.2, "a 3:1 day averages 2/3 of its peak");
    let count = |a: f64, b: f64| entries.iter().filter(|e| e.t >= a && e.t < b).count() as f64;
    near(count(540.0, 660.0) / count(0.0, 60.0).max(1.0) / 2.0, 3.0, 0.45, "peak over trough");

    let b = profile(&format!("{FLAT}\n[traffic.bursts]\nfactor = 2.0\nbusy_s = 10.0\ncalm_s = 60.0\n").replace("day_s = 600", "day_s = 3000"));
    let (bp, _, _) = plan(&b, 3, Shape::Day { rate: 30.0 }, "bursts");
    near(bp.stats.mean_rate, 30.0 * (1.0 + 10.0 / 70.0), 1.6, "bursts add their share");

    let f = profile(&format!("{FLAT}\n[[traffic.flash]]\nat_s = 100.0\nfor_s = 60.0\nfactor = 3.0\nqueries = 20\n"));
    let (fp, fe, _) = plan(&f, 4, Shape::Day { rate: 40.0 }, "flash");
    let during = fe.iter().filter(|e| e.t >= 100.0 && e.t < 160.0).count() as f64;
    near(during / 60.0, 120.0, 9.0, "three times the rate during the flash crowd");
    near(fp.stats.flash_requests as f64, 40.0 * 2.0 * 60.0, 300.0, "the extra requests are the story's");
    let story: BTreeSet<&str> = fe.iter().filter(|e| e.flash).filter_map(|e| e.qid.as_deref()).collect();
    assert!(story.len() <= 20 && story.len() >= 10, "{} queries of the story", story.len());
    assert!(fe.iter().filter(|e| e.flash).all(|e| e.t >= 100.0 && e.t < 160.0));
}

#[test]
fn sessions_follow_up_after_think_times() {
    let p = profile(&format!(
        "{}\n[sessions]\nfollow_up = 0.5\nthink_median_s = 4.0\nthink_sigma = 0.5\n",
        FLAT.replace("bm25 = 1\nvector = 1\n\"key lookup\" = 1", "bm25 = 1")
    ));
    let (plan, entries, _) = plan(&p, 5, Shape::Day { rate: 40.0 }, "sessions");
    let s = &plan.stats;
    near(s.requests_per_session, 2.0, 0.08, "steps a session (follow-up 0.5: 1 / (1 - 0.5))");
    near(s.requests as f64, 24_000.0, 900.0, "sessions arrive at the rate over their steps");
    let mut by: BTreeMap<u64, Vec<&Entry>> = BTreeMap::new();
    for e in entries.iter().filter(|e| e.s != 0) {
        by.entry(e.s).or_default().push(e);
    }
    assert!(!by.is_empty());
    let mut thinks = Vec::new();
    for steps in by.values() {
        assert!(steps.last().unwrap().last && steps.iter().filter(|e| e.last).count() == 1, "one last step");
        assert!(steps[0].think_ms.is_none());
        for w in steps.windows(2) {
            let think = w[1].think_ms.expect("a follow-up waits");
            near(w[1].t - w[0].t, think / 1e3, 2e-6, "planned a think time after the step before");
            thinks.push(think);
        }
        // A refinement is the same query with its filter.
        if let Some(r) = steps.iter().find(|e| e.k == "bm25 + filter") {
            assert_eq!(r.q, steps[0].q);
        }
    }
    thinks.sort_by(f64::total_cmp);
    near(thinks[thinks.len() / 2], 4000.0, 150.0, "think times' median");
    assert!(s.by_kind.contains_key("bm25 + filter") && s.by_kind.contains_key("key lookup") && s.by_kind.contains_key("similar items"));
}

#[test]
fn typing_sends_prefixes_at_pauses() {
    let p = profile(&format!("{}\n[sessions]\ntyping = true\nkey_gap_median_ms = 150.0\nkey_gap_sigma = 0.6\npause_ms = 250.0\n", FLAT.replace("vector = 1\n\"key lookup\" = 1\n", "")));
    let (plan, entries, _) = plan(&p, 6, Shape::Day { rate: 20.0 }, "typing");
    let prefixes: Vec<&Entry> = entries.iter().filter(|e| e.k == "prefix").collect();
    assert!(!prefixes.is_empty() && plan.stats.by_kind["prefix"] as usize == prefixes.len());
    for e in &prefixes {
        let k = e.knobs.expect("a prefix's length");
        let text = e.text.as_deref().unwrap();
        assert!(k.prefix_len >= 2 && prefix_of(text, k.prefix_len).is_some(), "{text:?} {}", k.prefix_len);
        // The query it is a prefix of comes later, from the same row.
        let main = entries.iter().find(|m| m.k == "bm25" && m.q == e.q && m.t >= e.t);
        assert!(main.is_some(), "the query typed after {text:?}");
        assert!(e.think_ms.is_none() && e.s == 0, "a prefix waits for nothing");
    }
}

#[test]
fn drawn_filters_keep_their_share_of_documents() {
    let p = profile(&format!(
        "{}\n[filters]\nmode = \"drawn\"\nmin_selectivity = 0.004\nmax_selectivity = 0.5\n",
        FLAT.replace("bm25 = 1\nvector = 1\n\"key lookup\" = 1", "\"bm25 + filter\" = 1\n\"vector + filter\" = 1\n\"similar items\" = 1")
    ));
    let (plan, entries, _) = plan(&p, 7, Shape::Day { rate: 10.0 }, "filters");
    // The documents, to count what each filter keeps.
    let docs: Vec<(i64, String, f64)> = std::fs::read_to_string(catalog().dir.join("docs.jsonl"))
        .unwrap()
        .lines()
        .map(|l| {
            let j: serde_json::Value = serde_json::from_str(l).unwrap();
            (j["year"].as_i64().unwrap(), j["category"].as_str().unwrap().to_string(), j["rating"].as_f64().unwrap())
        })
        .collect();
    let mut logs = Vec::new();
    for e in &entries {
        let (k, sel) = (e.knobs.expect("drawn knobs"), e.sel.unwrap());
        let kept = match e.k.as_str() {
            "bm25 + filter" => {
                let cats = k.cat_names();
                assert!(!cats.is_empty());
                docs.iter().filter(|d| d.0 >= k.year_min && cats.contains(&d.1)).count()
            }
            "vector + filter" => docs.iter().filter(|d| d.2 >= k.rating_min - 1e-9).count(),
            _ => docs.iter().filter(|d| d.2 >= k.similar_rating - 1e-9).count(),
        };
        near(sel, kept as f64 / DOCS as f64, 1e-12, &format!("{}: the share the filter keeps", e.k));
        if e.k == "bm25 + filter" {
            assert!((0.002..=0.75).contains(&sel), "{sel}");
            logs.push(sel.ln());
        }
    }
    // Log-uniform between the bounds: the median at their geometric mean.
    logs.sort_by(f64::total_cmp);
    near(logs[logs.len() / 2], ((0.004f64).ln() + (0.5f64).ln()) / 2.0, 0.35, "median log share");
    assert_eq!(plan.stats.selectivity["bm25 + filter"].len(), 5);
    // Knobs left at run's values are not written (fixed filters).
    let (_, fixed, _) = plan_fixed();
    assert!(fixed.iter().all(|e| e.knobs.is_none() && e.sel.is_none()));
}

fn plan_fixed() -> (Plan, Vec<Entry>, PathBuf) {
    plan(&profile(&FLAT.replace("bm25 = 1\n", "\"bm25 + filter\" = 1\n")), 1, Shape::Day { rate: 5.0 }, "fixed")
}

#[test]
fn writes_take_their_share_and_delete_only_their_own() {
    let p = profile(&format!(
        "{}\n[writes]\nshare = 0.2\ninsert = 0.5\nupdate = 0.4\ndelete = 0.1\nbatch_median = 4.0\nbatch_sigma = 0.7\nbatch_max = 50\nmark_every = 10\n\
         [[writes.jobs]]\nat_s = 60.0\ndocs = 1000\nbatch = 100\nclients = 2\n",
        FLAT
    ));
    let (plan, entries, _) = plan(&p, 8, Shape::Day { rate: 50.0 }, "writes");
    let writes: Vec<&Entry> = entries.iter().filter(|e| e.k == "write").collect();
    let (jobs, own): (Vec<&&Entry>, Vec<&&Entry>) = writes.iter().partition(|e| e.w.as_ref().unwrap().job.is_some());
    near(own.len() as f64 / plan.stats.requests as f64, 0.2, 0.015, "write share");
    let ops = |op: Op| own.iter().filter(|e| e.w.as_ref().unwrap().op == op).count() as f64 / own.len() as f64;
    near(ops(Op::Insert), 0.5, 0.03, "inserts");
    near(ops(Op::Update), 0.4, 0.03, "updates");
    let mut alive = BTreeSet::new();
    for e in &writes {
        let w = e.w.as_ref().unwrap();
        match w.op {
            Op::Insert => {
                assert_eq!(w.keys.len(), w.rows.len());
                assert!(w.keys.len() <= 100);
                assert!(w.rows.iter().all(|&r| r < 400), "rows of heldout.jsonl");
                for k in &w.keys {
                    assert!(alive.insert(k.clone()), "{k} inserted twice");
                }
            }
            Op::Delete => assert!(alive.remove(&w.keys[0]), "{} deleted but not inserted (or twice)", w.keys[0]),
            Op::Update => {
                assert_eq!(w.keys[0], format!("d{}", w.rows[0]));
                assert!((0.0..=5.0).contains(&w.rating.unwrap()));
            }
        }
    }
    let marks: BTreeSet<&str> = own.iter().filter_map(|e| e.w.as_ref().unwrap().mark.as_deref()).collect();
    near(marks.len() as f64, plan.stats.docs_inserted as f64 / 10.0, plan.stats.docs_inserted as f64 / 10.0 * 0.4 + 5.0, "a mark every 10 documents (one a batch at most)");
    // The job: 10 writes of 100 documents from 2 clients, each sending its next when the one before is answered.
    assert_eq!(jobs.len(), 10);
    assert_eq!(jobs.iter().map(|e| e.w.as_ref().unwrap().keys.len()).sum::<usize>(), 1000);
    let sessions: BTreeSet<u64> = jobs.iter().map(|e| e.s).collect();
    assert_eq!(sessions.len(), 2);
    assert_eq!(jobs.iter().filter(|e| e.think_ms.is_none()).count(), 2);
}

#[test]
fn the_warm_up_leaves_the_timed_requests_alone() {
    let p0 = profile(&FLAT.replace("warmup_s = 0", "warmup_s = 0.0"));
    let p30 = profile(&FLAT.replace("warmup_s = 0", "warmup_s = 30.0"));
    let (_, a, _) = plan(&p0, 9, Shape::Day { rate: 20.0 }, "warm-0");
    let (pb, b, _) = plan(&p30, 9, Shape::Day { rate: 20.0 }, "warm-30");
    let warm: Vec<&Entry> = b.iter().filter(|e| e.warm).collect();
    near(warm.len() as f64, 600.0, 90.0, "30 s of warm-up at 20 a second");
    assert!(warm.iter().all(|e| e.t < 30.0 + 1e-9) && pb.stats.warm_requests == warm.len() as u64);
    let shifted: Vec<Entry> = timed(&b).into_iter().map(|e| Entry { t: ((e.t - 30.0) * 1e6).round() / 1e6, ..e.clone() }).collect();
    let a: Vec<Entry> = a.into_iter().map(|e| Entry { t: (e.t * 1e6).round() / 1e6, ..e }).collect();
    assert_eq!(a.len(), shifted.len());
    assert!(a == shifted, "the timed requests are the same, 30 s later");
}

#[test]
fn closed_is_runs_requests() {
    let p = profile(
        r#"
name = "today"
[mix]
bm25 = 1
"key lookup" = 1
"similar items" = 1
"sql aggregate" = 1
[queries]
order = "cycle"
head = 300
tail_share = 0
[docs]
pick = "legacy"
"#,
    );
    let (plan, entries, _) = plan(&p, 1, Shape::Closed, "closed");
    assert_eq!(plan.stats.requests, 4 * 300);
    let n = DOCS.min(20_000);
    for (kind, list) in [("bm25", 0), ("key lookup", 1), ("similar items", 2), ("sql aggregate", 3)] {
        let mine: Vec<&Entry> = entries.iter().filter(|e| e.k == kind).collect();
        assert_eq!(mine.len(), 300);
        for (i, e) in mine.iter().enumerate() {
            // The i-th query with text: rows skip every tenth (no text).
            assert_eq!(e.q, Some((i + i / 9) as u64), "{kind} {i}");
            match list {
                1 => assert_eq!(e.row, Some(((i * 7919) % n) as u64)),
                2 => {
                    let it = (i * 104_729 + 13) % n;
                    assert_eq!(e.item, Some(it as u64));
                    assert_eq!(e.seen[1], format!("d{}", (it + 997) % n));
                    assert_eq!(e.seen.len(), 20);
                }
                3 => assert_eq!(e.year, Some(1990 + (i % 30) as i64)),
                _ => {}
            }
        }
    }
}

#[test]
fn ramp_steps_rise() {
    let p = profile(&format!("{FLAT}\n[traffic.ramp]\nstart = 20.0\nfactor = 2.0\nstep_s = 100.0\nsteps = 3\n"));
    let (plan, _, _) = plan(&p, 10, Shape::Ramp, "ramp");
    let s = &plan.stats.per_step;
    assert_eq!(s.len(), 3);
    for (i, want) in [2000.0, 4000.0, 8000.0].iter().enumerate() {
        near(s[i] as f64, *want, 4.0 * want.sqrt(), &format!("step {i}"));
    }
    assert_eq!(plan.header.steps[2].rate, 80.0);
}

#[test]
fn profiles_refuse_what_they_do_not_know() {
    assert!(Profile::parse("name = \"x\"\n[mix]\nbm25 = 1\n[queries]\nzipf = 1.0\n").unwrap_err().contains("zipf"));
    assert!(Profile::parse("name = \"x\"\n[mix]\nbm42 = 1\n").unwrap_err().contains("bm42"));
    assert!(Profile::parse("name = \"x\"\n[mix]\nbm25 = 0\n").is_err());
    assert!(Profile::parse("name = \"x\"\n[mix]\nbm25 = 1\n[writes]\nshare = 1.0\n").is_err());
    let p = Profile::parse("name = \"x\"\n[mix]\nbm25 = 1\n").unwrap();
    assert_eq!(p.queries.zipf_s, 0.8);
}

#[test]
fn kinds_resample_queries_they_cannot_ask() {
    let p = profile(&FLAT.replace("bm25 = 1\nvector = 1\n\"key lookup\" = 1", "phrase = 1\nprefix = 1\nfacet = 1"));
    let (_, entries, _) = plan(&p, 11, Shape::Day { rate: 10.0 }, "eligible");
    let ok = entries.iter().filter(|e| eligible(&e.k, e.text.as_deref().unwrap(), e.knobs.unwrap_or_default().prefix_len)).count();
    assert!(ok as f64 >= 0.99 * entries.len() as f64, "{ok} of {} can be asked", entries.len());
    let _ = Knobs::default();
}

/// The shipped profiles parse, and plan on the fixture.
#[test]
fn shipped_profiles_plan() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/compare/profiles");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap() {
        let path = f.unwrap().path();
        if path.extension().is_some_and(|x| x == "toml") {
            let (mut p, _) = super::read_profile(&path).unwrap();
            // Short, on the collection's own queries (the fixture has no pool).
            p.queries.source = super::profile::Source::Collection;
            p.traffic.day_s = 60.0;
            p.traffic.warmup_s = 5.0;
            p.traffic.flash.retain(|f| f.at_s < 50.0);
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let shape = if name == "today" { Shape::Closed } else { Shape::Day { rate: 20.0 } };
            plan(&p, 1, shape, &format!("shipped-{name}"));
            n += 1;
        }
    }
    assert!(n >= 3, "{n} profiles");
}
