//! A profile: every distribution of a workload, read from a TOML file
//! (`deploy/compare/profiles/*.toml`). Each field has a default, so a
//! profile names only what it changes; unknown fields are refused, so a
//! misspelt one cannot pass unnoticed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// What a session sets out to do: a query kind of `KINDS` and its
    /// weight. Sessions add steps of their own (prefixes while typing,
    /// refinements, opened results, similar items), so the kinds' shares
    /// of requests differ from these weights; `plan` prints them.
    pub mix: BTreeMap<String, f64>,
    #[serde(default)]
    pub queries: Queries,
    #[serde(default)]
    pub docs: Docs,
    #[serde(default)]
    pub filters: Filters,
    #[serde(default)]
    pub sessions: Sessions,
    #[serde(default)]
    pub traffic: Traffic,
    #[serde(default)]
    pub writes: Writes,
    #[serde(default)]
    pub slo: Slo,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The collection's own queries (`queries.jsonl`, those with text).
    Collection,
    /// The query pool `prepare_msmarco_compare.py --pool` writes beside the
    /// collection (`pool_queries.jsonl`, `pool_vecs.f32`).
    Pool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    /// Popularity: a head of `head` queries in a seeded random order, its
    /// ranks drawn by Zipf's law, and a tail of queries never sent before.
    Zipf,
    /// The first `head` queries in order, round-robin, as `run` sends them.
    Cycle,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TypoMode {
    /// A misspelt BM25 query is sent as the `typo` kind (fuzzy matching on).
    Fuzzy,
    /// A misspelt query is sent as its own kind, misspelt (an app that does
    /// not correct typos).
    Plain,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Queries {
    pub source: Source,
    pub order: Order,
    /// Queries of the head (capped by what the pool holds once the tail's reserve is set aside).
    pub head: usize,
    /// Zipf's exponent over the head's ranks.
    pub zipf_s: f64,
    /// Share of query draws that take a query never sent before in the run.
    pub tail_share: f64,
    /// Every this many seconds a slice of the head swaps with fresh queries (0: never).
    pub drift_every_s: f64,
    /// The share of the head's ranks that swap at each drift.
    pub drift_share: f64,
    /// Share of text queries that come misspelt (one transposition, as the `typo` kind makes).
    pub typo_share: f64,
    pub typos: TypoMode,
}

impl Default for Queries {
    fn default() -> Self {
        Queries { source: Source::Collection, order: Order::Zipf, head: 100_000, zipf_s: 0.8, tail_share: 0.5, drift_every_s: 300.0, drift_share: 0.02, typo_share: 0.0, typos: TypoMode::Fuzzy }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DocPick {
    /// By popularity: `doc_popularity.f32` beside the collection when it is
    /// there, else Zipf's law over a seeded random order of the documents.
    Popular,
    /// As `run` picks them: fixed strides over the first 20,000 documents.
    Legacy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Docs {
    pub pick: DocPick,
    /// Zipf's exponent of document popularity (key lookups, similar items, updates).
    pub zipf_s: f64,
}

impl Default for Docs {
    fn default() -> Self {
        Docs { pick: DocPick::Popular, zipf_s: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FilterMode {
    /// Each filtered kind's filter as `run` sends it.
    Fixed,
    /// A selectivity drawn log-uniform between `min_selectivity` and
    /// `max_selectivity` for each filtered request, and field values that
    /// hit it, measured on the collection's documents.
    Drawn,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Filters {
    pub mode: FilterMode,
    pub min_selectivity: f64,
    pub max_selectivity: f64,
}

impl Default for Filters {
    fn default() -> Self {
        Filters { mode: FilterMode::Fixed, min_selectivity: 0.0001, max_selectivity: 0.5 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sessions {
    /// After each answered step, the chance that the user takes another.
    pub follow_up: f64,
    /// Weights of the next step: the same query with a filter added (kinds
    /// that have a filtered twin), a result opened (key lookup), the items
    /// like an opened one (similar items).
    pub refine: f64,
    pub open: f64,
    pub similar: f64,
    /// Time between an answer and the next step, log-normal.
    pub think_median_s: f64,
    pub think_sigma: f64,
    /// Search as you type: the last word typed letter by letter before a
    /// text query, a `prefix` request at each pause between keys longer
    /// than `pause_ms`; the keys' gaps are log-normal.
    pub typing: bool,
    pub key_gap_median_ms: f64,
    pub key_gap_sigma: f64,
    pub pause_ms: f64,
}

impl Default for Sessions {
    fn default() -> Self {
        Sessions { follow_up: 0.0, refine: 1.0, open: 1.0, similar: 1.0, think_median_s: 5.0, think_sigma: 1.0, typing: false, key_gap_median_ms: 180.0, key_gap_sigma: 0.5, pause_ms: 100.0 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Bursts {
    /// The rate's factor while busy.
    pub factor: f64,
    /// Mean seconds in each state (exponential).
    pub busy_s: f64,
    pub calm_s: f64,
}

impl Default for Bursts {
    fn default() -> Self {
        Bursts { factor: 2.0, busy_s: 10.0, calm_s: 60.0 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flash {
    /// Seconds into the day.
    pub at_s: f64,
    pub for_s: f64,
    /// The rate's factor; the extra sessions all ask about one story.
    pub factor: f64,
    /// Queries of the story: the nearest this many to a seeded query of the head.
    #[serde(default = "default_flash_queries")]
    pub queries: usize,
}

fn default_flash_queries() -> usize {
    200
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ramp {
    /// Requests a second of the first step, each next step's factor, the steps' length and their most.
    pub start: f64,
    pub factor: f64,
    pub step_s: f64,
    pub steps: usize,
}

impl Default for Ramp {
    fn default() -> Self {
        Ramp { start: 50.0, factor: 1.25, step_s: 60.0, steps: 30 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Closed {
    /// Clients sending back to back, each count in turn, for `seconds`, after a warm-up (as `run`).
    pub concurrency: Vec<usize>,
    pub seconds: f64,
}

impl Default for Closed {
    fn default() -> Self {
        Closed { concurrency: vec![1, 4, 16], seconds: 6.0 }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Traffic {
    /// The compressed day: its length, the peak's rate over the trough's
    /// (1: flat), its humps and where (as a share of the day) the first
    /// peaks.
    pub day_s: f64,
    pub peak_to_trough: f64,
    pub humps: u32,
    pub peak_at: f64,
    pub bursts: Option<Bursts>,
    pub flash: Vec<Flash>,
    /// Seconds of warm-up before the timed schedule, drawn from another
    /// seed (page and filter caches warm up, the timed requests are not
    /// the warm-up's).
    pub warmup_s: f64,
    pub ramp: Ramp,
    pub closed: Closed,
}

impl Default for Traffic {
    fn default() -> Self {
        Traffic { day_s: 2880.0, peak_to_trough: 1.0, humps: 1, peak_at: 0.67, bursts: None, flash: Vec::new(), warmup_s: 120.0, ramp: Ramp::default(), closed: Closed::default() }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// Seconds into the run; the documents, a request's and the clients sending them back to back.
    pub at_s: f64,
    pub docs: u32,
    pub batch: u32,
    #[serde(default = "one")]
    pub clients: u32,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Writes {
    /// Share of all requests that are writes (0: none).
    pub share: f64,
    /// Weights of a write's kind: new documents, one field of a document
    /// changed (by popularity), a document of this run's deleted (oldest first).
    pub insert: f64,
    pub update: f64,
    pub delete: f64,
    /// Documents an insert carries: log-normal, at most `batch_max`.
    pub batch_median: f64,
    pub batch_sigma: f64,
    pub batch_max: u32,
    /// Every this many inserted documents one carries a word no other has,
    /// and a prober times how soon a search finds it (0: none).
    pub mark_every: u32,
    pub jobs: Vec<Job>,
}

impl Default for Writes {
    fn default() -> Self {
        Writes { share: 0.0, insert: 0.5, update: 0.45, delete: 0.05, batch_median: 1.0, batch_sigma: 0.0, batch_max: 1000, mark_every: 20, jobs: Vec::new() }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Slo {
    /// The reads' p99 (from each request's scheduled time) and share of errors a step may have and still hold.
    pub p99_ms: f64,
    pub errors: f64,
}

impl Default for Slo {
    fn default() -> Self {
        Slo { p99_ms: 100.0, errors: 0.001 }
    }
}

impl Profile {
    pub fn parse(text: &str) -> Result<Profile, String> {
        let p: Profile = toml::from_str(text).map_err(|e| format!("profile: {e}"))?;
        p.check()?;
        Ok(p)
    }

    fn check(&self) -> Result<(), String> {
        let known: Vec<&str> = super::KINDS.iter().map(|(k, _)| *k).collect();
        if self.mix.is_empty() || self.mix.values().all(|w| *w <= 0.0) {
            return Err("profile: mix needs a kind with a weight above 0".into());
        }
        for (k, w) in &self.mix {
            if !known.contains(&k.as_str()) {
                return Err(format!("profile: mix names {k:?}, not a kind (the kinds: {})", known.join(", ")));
            }
            if !w.is_finite() || *w < 0.0 {
                return Err(format!("profile: mix weight of {k:?} must be 0 or more"));
            }
        }
        let share = |name: &str, x: f64| if (0.0..=1.0).contains(&x) { Ok(()) } else { Err(format!("profile: {name} must be between 0 and 1, not {x}")) };
        share("queries.tail_share", self.queries.tail_share)?;
        share("queries.drift_share", self.queries.drift_share)?;
        share("queries.typo_share", self.queries.typo_share)?;
        share("sessions.follow_up", self.sessions.follow_up)?;
        if self.sessions.follow_up > 0.95 {
            return Err("profile: sessions.follow_up above 0.95 makes sessions of 20 steps and more on average".into());
        }
        share("writes.share", self.writes.share)?;
        if self.writes.share >= 1.0 {
            return Err("profile: writes.share must leave room for reads (below 1)".into());
        }
        if self.queries.zipf_s < 0.0 || self.docs.zipf_s < 0.0 {
            return Err("profile: Zipf exponents must be 0 or more".into());
        }
        if !(self.filters.min_selectivity > 0.0 && self.filters.min_selectivity <= self.filters.max_selectivity && self.filters.max_selectivity <= 1.0) {
            return Err("profile: filters need 0 < min_selectivity <= max_selectivity <= 1".into());
        }
        if self.traffic.peak_to_trough < 1.0 || self.traffic.day_s <= 0.0 {
            return Err("profile: traffic.peak_to_trough must be 1 or more and day_s above 0".into());
        }
        if let Some(b) = &self.traffic.bursts {
            if b.factor < 1.0 || b.busy_s <= 0.0 || b.calm_s <= 0.0 {
                return Err("profile: bursts need factor >= 1 and dwell times above 0".into());
            }
        }
        for f in &self.traffic.flash {
            if f.factor < 1.0 || f.for_s <= 0.0 || f.queries == 0 {
                return Err("profile: a flash crowd needs factor >= 1, for_s above 0 and queries".into());
            }
        }
        let r = &self.traffic.ramp;
        if r.start <= 0.0 || r.factor < 1.0 || r.step_s <= 0.0 || r.steps == 0 {
            return Err("profile: ramp needs start > 0, factor >= 1, step_s > 0 and steps".into());
        }
        if self.writes.share > 0.0 && self.writes.insert + self.writes.update + self.writes.delete <= 0.0 {
            return Err("profile: writes need a weight for insert, update or delete".into());
        }
        if self.writes.batch_max == 0 || self.writes.batch_median < 1.0 {
            return Err("profile: writes.batch_median must be 1 or more and batch_max above 0".into());
        }
        Ok(())
    }
}
