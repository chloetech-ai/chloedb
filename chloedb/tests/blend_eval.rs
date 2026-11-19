//! Blends valued off-policy against a world whose truth is known: an
//! exploring blend serves searches and logs them ([`Db::query_logged`]),
//! simulated people click by a position-based model whose chances are
//! known, and cascade doubly robust and SNIPS value other blends from the
//! log alone. Each estimate is checked against the exact value the blend
//! would have got: its own page for each search, every slot's click
//! chance summed.

use std::collections::HashMap;

use chloedb::db::{Db, SqlResult};
use chloedb::explore::{By, Policy};
use chloedb::query::blend_eval::{self, Options, Target, TuneOptions};
use chloedb::query::Params;
use chloedb::rank_log::{Events, Export, Log};
use chloedb::schema::Schema;
use chloedb::storage::{DbOptions, Layout};
use chloedb::value::Value;
use rand::{Rng, SeedableRng};

fn opts() -> DbOptions {
    DbOptions { sync: false, layout: Layout::SingleFile, segment_max_docs: 512, ..DbOptions::default() }
}

fn run(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

/// Searches (groups of rows) in the world.
const GROUPS: usize = 120;
/// Rows of each kind per group.
const PER: usize = 12;

/// The blend that logs, and the two valued against it.
const LOGGING: &str = "CREATE BLEND feed AS
  SOURCES (
    news   AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'news' ORDER BY rel DESC LIMIT 12),
    videos AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'video' ORDER BY rel DESC LIMIT 12)
  )
  HEADS (relevance CALIBRATE minmax, engagement CALIBRATE minmax)
  UTILITY 1 * relevance + 0.3 * engagement
  QUOTA (kind = 'video' AT LEAST 25% IN TOP 8)
  MAX 3 PER author
  WITH (k => 8)";

/// Engagement weighed up: the world's clicks follow engagement more.
const ENGAGING: &str = "CREATE BLEND engaging AS
  SOURCES (
    news   AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'news' ORDER BY rel DESC LIMIT 12),
    videos AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'video' ORDER BY rel DESC LIMIT 12)
  )
  HEADS (relevance CALIBRATE minmax, engagement CALIBRATE minmax)
  UTILITY 1 * relevance + 0.8 * engagement
  QUOTA (kind = 'video' AT LEAST 25% IN TOP 8)
  MAX 3 PER author
  WITH (k => 8)";

/// News only at the top: the videos people like are pushed down.
const NEWSY: &str = "CREATE BLEND newsy AS
  SOURCES (
    news   AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'news' ORDER BY rel DESC LIMIT 12),
    videos AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'video' ORDER BY rel DESC LIMIT 12)
  )
  HEADS (relevance CALIBRATE minmax, engagement CALIBRATE minmax)
  UTILITY 1 * relevance + 0.3 * engagement
  BOOST (news 0.25)
  MAX 3 PER author
  WITH (k => 8)";

/// Engagement only: far from the logging blend, its pages often leave
/// what the exploring blend could draw.
const FAR: &str = "CREATE BLEND far AS
  SOURCES (
    news   AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'news' ORDER BY rel DESC LIMIT 12),
    videos AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'video' ORDER BY rel DESC LIMIT 12)
  )
  HEADS (relevance CALIBRATE minmax, engagement CALIBRATE minmax)
  UTILITY 0.1 * relevance + 1 * engagement
  WITH (k => 8)";

/// The chance a person looks at slot `l`.
fn look(l: usize) -> f64 {
    1.0 / (l as f64).powf(0.8)
}

struct World {
    db: Db,
    /// Each row's chance of a click once looked at.
    appeal: HashMap<String, f64>,
    _dir: tempfile::TempDir,
}

fn world(seed: u64) -> World {
    let dir = tempfile::tempdir().unwrap();
    let s = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("w.chloedb"), s, opts()).unwrap();
    run(&db, "CREATE TABLE docs (id KEYWORD PRIMARY KEY, grp KEYWORD, kind KEYWORD, author KEYWORD, rel FLOAT, pop FLOAT)");
    run(&db, "CREATE TABLE clicks (id KEYWORD PRIMARY KEY, usr KEYWORD, item KEYWORD, ts TIMESTAMP)");
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut appeal = HashMap::new();
    let mut values = Vec::new();
    for g in 0..GROUPS {
        for kind in ["news", "video"] {
            for i in 0..PER {
                let id = format!("g{g}-{kind}-{i}");
                let (rel, pop): (f64, f64) = (rng.gen(), rng.gen());
                // People click on what engages them, videos a little more.
                let a = (0.08 + 0.45 * pop * (0.4 + 0.6 * rel) + if kind == "video" { 0.06 } else { 0.0 }).min(0.95);
                appeal.insert(id.clone(), a);
                values.push(format!("('{id}', 'g{g}', '{kind}', 'a{}', {rel}, {pop})", rng.gen_range(0..6)));
            }
        }
    }
    for chunk in values.chunks(400) {
        run(&db, &format!("INSERT INTO docs VALUES {}", chunk.join(", ")));
    }
    for d in [LOGGING, ENGAGING, NEWSY, FAR] {
        run(&db, d);
    }
    World { db, appeal, _dir: dir }
}

fn group(g: usize) -> Params {
    Params::new().set("g", Value::Str(format!("g{g}")))
}

/// A blend's page for group `g`, the best taken: its row keys.
fn page(db: &Db, blend: &str, g: usize) -> Vec<String> {
    match db.sql(&format!("SELECT id FROM blend('{blend}', g => :g)"), &group(g)).unwrap() {
        SqlResult::Rows(r) => r.rows.iter().map(|row| match &row[0] {
            Value::Str(s) => s.clone(),
            v => format!("{v:?}"),
        }).collect(),
        SqlResult::Written(_) => Vec::new(),
    }
}

/// A page's exact value: each slot's click chance, summed.
fn worth(w: &World, keys: &[String]) -> f64 {
    keys.iter().enumerate().map(|(l, k)| look(l + 1) * w.appeal[k]).sum()
}

const T0: i64 = 1_700_000_000_000_000;

/// The rows each slot's draw spreads over.
const TOP: usize = 6;

/// Serves `n` exploring searches of the logging blend, logged, and the
/// clicks people make on them.
fn serve(w: &World, n: usize, seed: u64, tau: f64) -> Vec<usize> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut groups = Vec::with_capacity(n);
    let mut clicks = Vec::new();
    for i in 0..n {
        let g = rng.gen_range(0..GROUPS);
        groups.push(g);
        let request = format!("r{seed}-{i}");
        let ts = T0 + i as i64 * 10_000_000;
        let log = Log { request: Some(request.clone()), context: Some(request.clone()), ts: Some(ts), explore: Some(Policy::Softmax { tau, top: TOP, by: By::Score }), ..Log::default() };
        let got = w.db.query_logged("SELECT id FROM blend('feed', g => :g)", &group(g), &log).unwrap();
        assert_eq!(got.propensities.len(), got.rs.rows.len());
        for (l, row) in got.rs.rows.iter().enumerate() {
            let Value::Str(key) = &row[0] else { panic!("a key is text") };
            if rng.gen::<f64>() < look(l + 1) * w.appeal[key] {
                clicks.push(vec![Value::Str(format!("{request}-{l}")), Value::Str(request.clone()), Value::Str(key.clone()), Value::Timestamp(ts + 1_000_000)]);
            }
        }
    }
    let mut t = w.db.storage().begin().unwrap();
    {
        let mut tt = t.table("clicks").unwrap();
        for c in clicks {
            tt.insert(c).unwrap();
        }
    }
    t.commit().unwrap();
    groups
}

fn export() -> Export {
    Export {
        events: Some(Events { table: "clicks".into(), user: "usr".into(), item: "item".into(), time: "ts".into(), action: None }),
        window: Some(60_000_000),
        ..Export::default()
    }
}

fn target(db: &Db, name: &str) -> Target {
    let (def, v) = chloedb::query::blend::resolve(&*db.functions().unwrap(), name, None).unwrap();
    Target { label: format!("{name}@{v}"), def, explore: None }
}

#[test]
fn cascade_doubly_robust_and_snips_find_each_blends_true_value() {
    let w = world(7);
    let n = 4000;
    let groups = serve(&w, n, 11, 0.4);
    let gathered = w.db.blend_requests(&export(), "feed").unwrap();
    assert_eq!(gathered.requests.len(), n, "every logged search explored: {} not exploring, {} broken", gathered.not_exploring, gathered.broken);
    let mut reqs = gathered.requests;

    // The logging blend replayed as it served: every chance comes back,
    // so every weight is 1 and every slot has every search's support.
    let (feed, _) = chloedb::query::blend::resolve(&*w.db.functions().unwrap(), "feed", None).unwrap();
    let own = Target { label: "feed as it explored".into(), def: feed.clone(), explore: reqs[0].explore };
    let versions = blend_eval::versions(&w.db.functions().unwrap(), "feed");
    let logging = |v: i64| versions.get(&v).cloned();
    let (est, _) = blend_eval::evaluate(&mut reqs, &logging, &[own], Options::default()).unwrap();
    for s in &est[0].slots {
        assert!((s.ess - n as f64).abs() < 1e-6 * n as f64, "slot {} has {} effective searches of {n}: a chance did not come back", s.slot, s.ess);
    }
    assert!((est[0].snips.value - est[0].logged.value).abs() < 1e-9, "weights of 1 give back the logged value");

    // Each blend, the best taken, against its exact value over the same
    // searches.
    let mut truth = HashMap::new();
    let mut slot_truth = HashMap::new();
    for name in ["feed", "engaging", "newsy", "far"] {
        let pages: Vec<Vec<String>> = (0..GROUPS).map(|g| page(&w.db, name, g)).collect();
        let per: Vec<f64> = pages.iter().map(|p| worth(&w, p)).collect();
        truth.insert(name, groups.iter().map(|&g| per[g]).sum::<f64>() / n as f64);
        let by_slot: Vec<f64> = (0..8).map(|l| groups.iter().map(|&g| pages[g].get(l).map_or(0.0, |k| look(l + 1) * w.appeal[k])).sum::<f64>() / n as f64).collect();
        slot_truth.insert(name, by_slot);
    }
    let targets: Vec<Target> = ["feed", "engaging", "newsy", "far"].iter().map(|t| target(&w.db, t)).collect();
    let (est, model) = blend_eval::evaluate(&mut reqs, &logging, &targets, Options::default()).unwrap();
    assert!(model["reads"].as_array().is_some_and(|r| !r.is_empty()), "the click model reads the rows' numbers: {model}");
    for (e, name) in est.iter().zip(["feed", "engaging", "newsy", "far"]) {
        let t = truth[name];
        eprintln!(
            "{name}: truth {t:.4}, cdr {:.4} ± {:.4}, top {} slots: cdr {:.4} ± {:.4} snips {:.4} ± {:.4}; model {:.4}, logged {:.4}, support {:.0}, coverage {:.3}, switched {:.3}, {} ({})",
            e.cdr.value, 1.96 * e.cdr.se, e.check_depth, e.cdr_top.value, 1.96 * e.cdr_top.se, e.snips.value, 1.96 * e.snips.se, e.model, e.logged.value, e.support, e.coverage, e.switched, e.verdict, e.why
        );
        let per_slot = &slot_truth[name];
        for s in &e.slots {
            eprintln!("  slot {}: truth {:.4} cdr {:.4} snips {:.4} model {:.4} logged {:.4} ess {:.0} cov {:.3}", s.slot, per_slot[s.slot - 1], s.cdr, s.snips, s.model, s.logged, s.ess, s.coverage);
        }
        if name == "far" {
            continue;
        }
        assert!(e.coverage >= blend_eval::MIN_COVERAGE, "{name}: coverage {}", e.coverage);
        // Unbiased: within four standard errors (and a hair) of the truth.
        assert!((e.cdr.value - t).abs() <= 4.0 * e.cdr.se + 1e-3, "{name}: cascade doubly robust {} ± {} against the truth {t}", e.cdr.value, e.cdr.se);
        // SNIPS over the top slots it has the searches for, against the
        // truth there, and cascade doubly robust over the same slots no
        // wider.
        let top: f64 = per_slot[..e.check_depth].iter().sum();
        assert!(e.check_depth >= 2, "{name}: SNIPS has the searches for {} slots", e.check_depth);
        assert!((e.snips.value - top).abs() <= 4.0 * e.snips.se + 1e-3, "{name}: SNIPS {} ± {} against the truth {top} over the top {} slots", e.snips.value, e.snips.se, e.check_depth);
        assert!(e.cdr_top.se <= e.snips.se * 1.05, "{name}: cascade doubly robust's interval ({}) is no wider than SNIPS's ({})", e.cdr_top.se, e.snips.se);
    }
    // The world rewards engagement: the estimates order the blends as the
    // truth does, and engaging is called better than the log.
    assert!(truth["engaging"] > truth["feed"] && truth["feed"] > truth["newsy"], "the world as built: {truth:?}");
    assert!(est[1].cdr.value > est[0].cdr.value && est[0].cdr.value > est[2].cdr.value);
    // Each judged against the first, feed as it is: engaging better,
    // newsy worse, far not proven (most of its worth lies where the log
    // could not draw).
    assert_eq!(est[1].verdict, "better", "{}", est[1].why);
    assert_eq!(est[2].verdict, "worse", "{}", est[2].why);
    assert_eq!(est[3].verdict, "not proven", "{}", est[3].why);
    assert!(est[3].why.contains("could not draw"), "{}", est[3].why);
    let truth_lift = truth["engaging"] - truth["feed"];
    assert!((est[1].lift.value - truth_lift).abs() <= 4.0 * est[1].lift.se + 1e-3, "engaging's lift {} ± {} against the truth's {truth_lift}", est[1].lift.value, est[1].lift.se);
}

#[test]
fn tuning_finds_a_better_blend_and_proves_it_on_the_other_half() {
    let w = world(3);
    let n = 4000;
    let groups = serve(&w, n, 5, 0.4);
    let mut reqs = w.db.blend_requests(&export(), "feed").unwrap().requests;
    let start = target(&w.db, "feed");
    let versions = blend_eval::versions(&w.db.functions().unwrap(), "feed");
    let tuned = blend_eval::tune(&mut reqs, &|v| versions.get(&v).cloned(), &start, TuneOptions { rounds: 16, seed: 9, discount: false }).unwrap();
    assert_eq!(tuned.tried.len(), 17, "the start and 16 blends near it");
    let (best, held_out) = tuned.best.clone().unwrap();
    let start_held = tuned.start.clone().unwrap();
    eprintln!("tuned: {} held out {:.4} ± {:.4} against the start's {:.4} ({})", best.note.clone().unwrap_or_default(), held_out.cdr.value, 1.96 * held_out.cdr.se, start_held.cdr.value, held_out.verdict);
    // The best one's true value, over the held-out searches.
    let group_of = |id: &str| groups[id.rsplit('-').next().unwrap().parse::<usize>().unwrap()];
    let held: Vec<usize> = reqs.iter().filter(|r| r.fold & 1 == 1).map(|r| group_of(&r.id)).collect();
    run(&w.db, &best.sql(true).replacen("BLEND feed", "BLEND tuned", 1));
    let per: Vec<f64> = (0..GROUPS).map(|g| worth(&w, &page(&w.db, "tuned", g))).collect();
    let per_start: Vec<f64> = (0..GROUPS).map(|g| worth(&w, &page(&w.db, "feed", g))).collect();
    let t_best = held.iter().map(|&g| per[g]).sum::<f64>() / held.len() as f64;
    let t_start = held.iter().map(|&g| per_start[g]).sum::<f64>() / held.len() as f64;
    assert!(t_best >= t_start, "tuning chose a blend no worse in truth: {t_best} against {t_start}");
    assert!((held_out.cdr.value - t_best).abs() <= 4.0 * held_out.cdr.se + 1e-3, "the held-out estimate {} ± {} against the truth {t_best}", held_out.cdr.value, held_out.cdr.se);
}
