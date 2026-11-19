//! A query's funnel: everything that happens between the text of a search
//! and the rows it answers, stage by stage, with the time each took and
//! the rows each kept and dropped.
//!
//! The stages ([`Stage`]), in the order a query meets them:
//!
//! 1. **Wait**: for a query slot, a thread, or the writes it must see.
//! 2. **Understand**: words that name values become conditions
//!    ([`crate::query::understand`]).
//! 3. **Expand**: the words become the terms searched: typos corrected,
//!    synonyms, prefixes, terms from the first rows (`expand =>`,
//!    [`crate::query::prf`]).
//! 4. **Embed**: texts where vectors go become vectors
//!    ([`crate::query::embed_sql`]).
//! 5. **Plan**: the statement parsed and bound to its table.
//! 6. **Match**: the rows each input of the ranking reaches: BM25's rows
//!    that hold a term, the nearest vectors, a column's values.
//! 7. **Select**: `WHERE`, and each input's top rows, which become the
//!    candidates the ranking scores.
//! 8. **Rank**: the candidates scored by the ranking expression, ordered
//!    and cut to the page.
//! 9. **Rerank**: rerank phases and cross-encoders over the top rows.
//! 10. **Blend**: `FROM blend(...)`: sources' rows placed in slots
//!     ([`crate::query::blend`]).
//! 11. **Shards**: on a coordinator, the calls to the shards: the network
//!     and what a shard does around its query.
//! 12. **Fetch**: the answer's columns read, rows counted or grouped, the
//!     answer handed back.
//! 13. **Other**: the server's own work around the query (reading the
//!     request, writing the answer) that no step names.
//!
//! **Time.** Every query records its steps ([`crate::query::steps`]) and a
//! server its request's spans; [`timings`] lays both on the stages, each
//! microsecond of the query's wall time counted once: a step's time is its
//! stage's, less the time of the steps inside it, which count for theirs;
//! steps that ran side by side (segments on several threads, shards called
//! at once) share the time they ran together evenly. A step
//! of no stage (a segment's) counts for the stage of the step it is in.
//! [`aggregate`] sums the timings of many queries.
//!
//! **Rows.** A query's rows at each stage come from probes: queries made
//! from the bound query ([`Probes::of`]), each an ordinary [`Select`] that
//! a database runs as any query and a coordinator over its shards as any
//! query, so that the funnel needs nothing of a shard but queries. The
//! first round counts the rows each input matches, the rows left after
//! each condition of `WHERE` in turn, and reads each input's top rows
//! without `WHERE` and with it, and the ranking past the page; the second
//! asks which of the rows seen pass each condition, so that a row `WHERE`
//! dropped says which condition dropped it. [`Probes::finish`] puts them
//! together ([`Rows`]): the counts, and each row seen with its fate, where
//! it left the funnel and why.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::query::exec::ResultSet;
use crate::query::logical::{OrderBy, Pred, Projection, Score, Select};
use crate::query::rank::Rank;
use crate::query::steps::Step;
use crate::schema::{FieldType, Schema};
use crate::value::Value;

// ---------------------------------------------------------------- stages

/// A stage of a query's funnel (the module's documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Wait,
    Understand,
    Expand,
    Embed,
    Plan,
    Match,
    Select,
    Rank,
    Rerank,
    Blend,
    Shards,
    Fetch,
    Other,
}

/// How many stages there are.
pub const STAGES: usize = 13;

impl Stage {
    /// Every stage, in the order a query meets them.
    pub const ALL: [Stage; STAGES] = [
        Stage::Wait,
        Stage::Understand,
        Stage::Expand,
        Stage::Embed,
        Stage::Plan,
        Stage::Match,
        Stage::Select,
        Stage::Rank,
        Stage::Rerank,
        Stage::Blend,
        Stage::Shards,
        Stage::Fetch,
        Stage::Other,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The stage's name, as the funnel shows it.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Wait => "Wait",
            Stage::Understand => "Understand",
            Stage::Expand => "Expand terms",
            Stage::Embed => "Embed",
            Stage::Plan => "Plan",
            Stage::Match => "Match",
            Stage::Select => "Select",
            Stage::Rank => "Rank",
            Stage::Rerank => "Rerank",
            Stage::Blend => "Blend",
            Stage::Shards => "Shards",
            Stage::Fetch => "Fetch",
            Stage::Other => "Other",
        }
    }

    /// What happens in it, in a line.
    pub fn says(self) -> &'static str {
        match self {
            Stage::Wait => "waiting for a query slot, a thread, or the writes the query must see",
            Stage::Understand => "words that name values of keyword fields, or ranges of numbers, become conditions",
            Stage::Expand => "the words become the terms searched: typos corrected, synonyms, prefixes, terms from the first rows",
            Stage::Embed => "texts where vectors go embedded by their model",
            Stage::Plan => "the statement parsed and bound to its table",
            Stage::Match => "the rows each input of the ranking reaches: rows that hold a term, the nearest vectors, a column's values",
            Stage::Select => "WHERE, and each input's top rows, which become the candidates the ranking scores",
            Stage::Rank => "the candidates scored by the ranking, ordered and cut to the page",
            Stage::Rerank => "rerank phases and cross-encoders over the top rows",
            Stage::Blend => "sources' rows placed in the slots of a blend",
            Stage::Shards => "the calls to the shards: the network, and a shard's work around its query",
            Stage::Fetch => "the answer's columns read, rows counted or grouped, the answer handed back",
            Stage::Other => "the server's own work around the query that no step names",
        }
    }

    /// The stage of an executor's step ([`Step::op`]), `None` for a step
    /// that counts for the step it is in (a segment's).
    pub fn of_step(op: &str) -> Option<Stage> {
        Some(match op {
            "wait for a thread" => Stage::Wait,
            "terms" => Stage::Expand,
            "bm25" | "vector" | "sparse" | "maxsim" | "distance" | "column" | "decay" | "graph" | "graphs" | "ranked" | "hash" | "scan" | "read by key" | "open readers" => Stage::Match,
            "filter" | "guard" | "candidates" => Stage::Select,
            "rank" | "order" | "exact scores" | "phase values" | "merge" | "score" => Stage::Rank,
            "rerank" | "cross-encoder" => Stage::Rerank,
            "blend" => Stage::Blend,
            "shard" | "on the shard" => Stage::Shards,
            "project" | "finish" | "hand back" | "answer cache" | "count" | "aggregate" | "facet" | "datafusion" => Stage::Fetch,
            _ => return None,
        })
    }

    /// The stage of a server's span (`telemetry::record`), `None` for one
    /// the steps hold (`execute`) or that is not a stage.
    pub fn of_span(name: &str) -> Option<Stage> {
        Some(match name {
            "admission" | "wait for writes" => Stage::Wait,
            "understand" => Stage::Understand,
            "expand" => Stage::Expand,
            "embed" => Stage::Embed,
            "plan" => Stage::Plan,
            "blend" => Stage::Blend,
            _ => return None,
        })
    }

    /// A stage whose span holds a whole query of its own (the rows an
    /// understanding or an expansion reads, a blend's sources): the spans
    /// inside it count for it.
    fn holds_queries(self) -> bool {
        matches!(self, Stage::Understand | Stage::Expand | Stage::Blend)
    }
}

// ---------------------------------------------------------------- time

/// A server's span of a request, as [`timings`] reads it.
#[derive(Clone, Debug)]
pub struct SpanTime<'a> {
    pub name: &'a str,
    pub start_us: f64,
    pub dur_us: f64,
    /// A call to a shard: its time is in the coordinator's `shard` steps.
    pub call: bool,
}

/// Microseconds per stage, by [`Stage::index`].
pub type Times = [f64; STAGES];

/// One node of a query's steps, as a tree.
struct Node {
    stage: Option<Stage>,
    start: f64,
    us: f64,
    children: Vec<usize>,
}

/// `steps` (pre-order, by depth) as a tree: the nodes and the roots.
fn tree(steps: &[Step]) -> (Vec<Node>, Vec<usize>) {
    let mut nodes: Vec<Node> = Vec::with_capacity(steps.len());
    let mut roots = Vec::new();
    // The open nodes by depth.
    let mut open: Vec<(u16, usize)> = Vec::new();
    for s in steps {
        while open.last().is_some_and(|&(d, _)| d >= s.depth) {
            open.pop();
        }
        let i = nodes.len();
        nodes.push(Node { stage: Stage::of_step(&s.op), start: s.start_us, us: s.us.max(0.0), children: Vec::new() });
        match open.last() {
            Some(&(_, p)) => nodes[p].children.push(i),
            None => roots.push(i),
        }
        open.push((s.depth, i));
    }
    (nodes, roots)
}

/// The time each of `spans` (start, length) holds of `[lo, hi]`: each
/// stretch of time split evenly among the spans that ran in it, so that
/// two side by side hold half of it each and one alone all of it. The
/// sum is the length of their union.
fn shares(spans: &[(f64, f64)], lo: f64, hi: f64) -> Vec<f64> {
    let clipped: Vec<(f64, f64)> = spans.iter().map(|&(s, len)| (s.max(lo), (s + len).min(hi))).collect();
    let mut cuts: Vec<f64> = clipped.iter().filter(|(a, b)| b > a).flat_map(|&(a, b)| [a, b]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut out = vec![0.0; spans.len()];
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let on: Vec<usize> = (0..clipped.len()).filter(|&i| clipped[i].0 <= a && clipped[i].1 >= b && clipped[i].1 > clipped[i].0).collect();
        for &i in &on {
            out[i] += (b - a) / on.len() as f64;
        }
    }
    out
}

/// Lays node `i`'s time on the stages, `scale` of it: its own time (less
/// what its children held of it) to its stage, or `inherited`'s, and each
/// child's share of the rest to the child's ([`shares`]).
fn lay(nodes: &[Node], i: usize, inherited: Stage, scale: f64, out: &mut Times) {
    let n = &nodes[i];
    let stage = n.stage.unwrap_or(inherited);
    let kids: Vec<(f64, f64)> = n.children.iter().map(|&c| (nodes[c].start, nodes[c].us)).collect();
    let held = shares(&kids, n.start, n.start + n.us);
    let inside: f64 = held.iter().sum();
    out[stage.index()] += (n.us - inside).max(0.0) * scale;
    for (&c, h) in n.children.iter().zip(held) {
        if nodes[c].us > 0.0 {
            lay(nodes, c, stage, scale * h / nodes[c].us, out);
        }
    }
}

/// The time of a query's steps on the stages: the wall time from the
/// first step's start to the last's end, the gaps between steps at the top
/// [`Stage::Other`].
pub fn step_times(steps: &[Step]) -> Times {
    let mut out = [0.0; STAGES];
    let (nodes, roots) = tree(steps);
    if roots.is_empty() {
        return out;
    }
    let lo = roots.iter().map(|&r| nodes[r].start).fold(f64::INFINITY, f64::min);
    let hi = roots.iter().map(|&r| nodes[r].start + nodes[r].us).fold(f64::NEG_INFINITY, f64::max);
    let held = shares(&roots.iter().map(|&r| (nodes[r].start, nodes[r].us)).collect::<Vec<_>>(), lo, hi);
    out[Stage::Other.index()] += (hi - lo - held.iter().sum::<f64>()).max(0.0);
    for (&r, h) in roots.iter().zip(held) {
        if nodes[r].us > 0.0 {
            lay(&nodes, r, Stage::Other, h / nodes[r].us, &mut out);
        }
    }
    out
}

/// The time of one query on the stages: its steps' ([`step_times`]) and
/// its spans' (understanding, expansion, embedding, planning, waiting),
/// `total_us` its wall time, what neither holds [`Stage::Other`]. A span
/// inside one that holds queries of its own (an understanding's reads)
/// counts for it; `plan` counts less the embedding inside it; `execute`
/// and calls to shards are in the steps, and count when there are none.
pub fn timings(steps: &[Step], spans: &[SpanTime<'_>], total_us: f64) -> Times {
    let mut out = step_times(steps);
    let holders: Vec<(f64, f64)> = spans.iter().filter(|s| Stage::of_span(s.name).is_some_and(Stage::holds_queries)).map(|s| (s.start_us, s.start_us + s.dur_us)).collect();
    let within_holder = |s: &SpanTime<'_>| holders.iter().any(|&(a, b)| s.start_us >= a && s.start_us + s.dur_us <= b && !(s.start_us == a && s.start_us + s.dur_us == b));
    for s in spans {
        let stage = Stage::of_span(s.name);
        if within_holder(s) && !stage.is_some_and(Stage::holds_queries) {
            continue;
        }
        match stage {
            Some(Stage::Plan) => {
                let embedded: f64 = spans.iter().filter(|e| e.name == "embed" && e.start_us >= s.start_us && e.start_us + e.dur_us <= s.start_us + s.dur_us).map(|e| e.dur_us).sum();
                out[Stage::Plan.index()] += (s.dur_us - embedded).max(0.0);
            }
            Some(st) => out[st.index()] += s.dur_us,
            None if steps.is_empty() && s.call => out[Stage::Shards.index()] += s.dur_us,
            None => {}
        }
    }
    let known: f64 = out.iter().sum();
    if total_us > known {
        out[Stage::Other.index()] += total_us - known;
    }
    out
}

/// One stage's time over many queries ([`aggregate`]).
#[derive(Clone, Debug, Serialize)]
pub struct StageTime {
    pub stage: Stage,
    pub name: &'static str,
    pub says: &'static str,
    /// The queries in which it took any time.
    pub ran: u64,
    /// Its mean over every query (0 where it did not run), and its
    /// percentiles over those that ran it, in milliseconds.
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    /// Its share of the queries' summed wall time.
    pub share: f64,
}

/// The time of many queries on the stages.
#[derive(Clone, Debug, Serialize)]
pub struct Aggregate {
    pub queries: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub stages: Vec<StageTime>,
}

/// The `q` quantile of sorted `v`, 0 for none.
fn quantile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let i = ((v.len() - 1) as f64 * q).round() as usize;
    v[i.min(v.len() - 1)]
}

fn ms(us: f64) -> f64 {
    (us / 1e3 * 1000.0).round() / 1000.0
}

/// The stages' time over queries `each`, each a query's [`Times`] and its
/// wall time in microseconds. A stage under a microsecond did not run.
pub fn aggregate(each: &[(Times, f64)]) -> Aggregate {
    let n = each.len();
    let mut totals: Vec<f64> = each.iter().map(|(_, t)| *t).collect();
    totals.sort_by(f64::total_cmp);
    let sum_total: f64 = totals.iter().sum();
    let stages = Stage::ALL
        .iter()
        .map(|&st| {
            let mut ran: Vec<f64> = each.iter().map(|(t, _)| t[st.index()]).filter(|&x| x >= 1.0).collect();
            ran.sort_by(f64::total_cmp);
            let sum: f64 = ran.iter().sum();
            StageTime {
                stage: st,
                name: st.name(),
                says: st.says(),
                ran: ran.len() as u64,
                mean_ms: if n == 0 { 0.0 } else { ms(sum / n as f64) },
                p50_ms: ms(quantile(&ran, 0.5)),
                p95_ms: ms(quantile(&ran, 0.95)),
                max_ms: ms(ran.last().copied().unwrap_or(0.0)),
                share: if sum_total > 0.0 { ((sum / sum_total) * 1e4).round() / 1e4 } else { 0.0 },
            }
        })
        .collect();
    Aggregate { queries: n as u64, mean_ms: if n == 0 { 0.0 } else { ms(sum_total / n as f64) }, p50_ms: ms(quantile(&totals, 0.5)), p95_ms: ms(quantile(&totals, 0.95)), stages }
}

/// [`Times`] as a JSON object of milliseconds by stage, the stages that
/// took no time left out.
pub fn times_json(t: &Times) -> serde_json::Value {
    serde_json::Value::Object(Stage::ALL.iter().filter(|s| t[s.index()] >= 1.0).map(|s| (stage_key(*s), serde_json::json!(ms(t[s.index()])))).collect())
}

/// A stage's key in JSON (`match`, `rerank`, ...).
pub fn stage_key(s: Stage) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The rows a query's steps say each stage ended with: the most rows a
/// step of each stage held. For the funnel over many queries, where no
/// probe ran.
pub fn step_rows(steps: &[Step]) -> BTreeMap<Stage, u64> {
    let mut out = BTreeMap::new();
    let (nodes, roots) = tree(steps);
    fn walk(nodes: &[Node], steps: &[Step], i: usize, inherited: Stage, out: &mut BTreeMap<Stage, u64>) {
        let st = nodes[i].stage.unwrap_or(inherited);
        if let (Some(r), Some(_)) = (steps[i].rows, nodes[i].stage) {
            let e = out.entry(st).or_insert(0);
            *e = (*e).max(r);
        }
        for &c in &nodes[i].children {
            walk(nodes, steps, c, st, out);
        }
    }
    for r in roots {
        walk(&nodes, steps, r, Stage::Other, &mut out);
    }
    out
}

// ---------------------------------------------------------------- rows

/// Rows past the page, and past each input's depth, that the probes read
/// to show what was cut there.
pub const PAST: usize = 20;

/// The most rows of each input's top the probes read.
pub const MAX_TOP: usize = 200;

/// The most conditions of `WHERE` counted one by one; the rest count as
/// one, the last.
pub const MAX_CONDITIONS: usize = 10;

/// The most inputs of a ranking followed.
pub const MAX_INPUTS: usize = 8;

/// A probe of the funnel: a query to run, and the name its answer goes
/// by.
#[derive(Clone, Debug)]
pub struct Probe {
    pub name: String,
    pub select: Select,
}

/// An input of a query's ranking: a leaf score.
#[derive(Clone, Debug)]
struct Input {
    score: Score,
    label: String,
    kind: &'static str,
    /// Its top rows are candidates.
    generates: bool,
    /// The rows of its top that are candidates.
    depth: usize,
    /// The rows it matches, as a condition (`None`: not countable).
    matches: Option<Pred>,
    /// The text it searches, the field.
    text: Option<String>,
    field: Option<String>,
}

/// The probes of one query, and what [`Probes::finish`] needs to read
/// their answers.
#[derive(Clone, Debug)]
pub struct Probes {
    /// `search` (a ranking by scores), `rows` (an order by a column, or
    /// none), or `other` (groups, counts: no probes).
    pub kind: &'static str,
    q: Select,
    key: Option<usize>,
    key_name: Option<String>,
    inputs: Vec<Input>,
    /// A ranking expression over the inputs, and its text.
    fused: Option<String>,
    conds: Vec<Pred>,
    cond_text: Vec<String>,
    /// LIMIT + OFFSET.
    reach: usize,
    /// The rows of the ranking the probe reads.
    ranked_n: usize,
    phases: Vec<(String, usize)>,
}

/// Each row the funnel saw, and where it left.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub key: serde_json::Value,
    /// `shown` (on the page), `offset` (ranked before the page), `cut`
    /// (ranked past the page), `reranked` (past a rerank phase's rows),
    /// `not_candidate` (it passes WHERE but is in no input's top),
    /// `filtered` (WHERE dropped it).
    pub fate: &'static str,
    /// The stage it left at, or the last it reached.
    pub stage: Stage,
    pub why: String,
    /// Its place on the page, from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    /// Its place in the ranking, from 1, and its score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Per input: its place in the input's top without WHERE, and its
    /// value there; `None` where it is not in it.
    pub matched: Vec<Option<(usize, f64)>>,
    /// Per input: its place in the input's top among the rows WHERE keeps.
    pub selected: Vec<Option<usize>>,
    /// The conditions of WHERE it fails, by number.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fails: Vec<usize>,
}

/// An input of the ranking, as the funnel shows it.
#[derive(Clone, Debug, Serialize)]
pub struct InputRows {
    pub label: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub generates: bool,
    pub depth: usize,
    /// The rows it matches (`None`: not counted: the nearest vectors
    /// have no count of their own).
    pub matched: Option<u64>,
    /// The rows of its top among those WHERE keeps that are candidates.
    pub candidates: u64,
}

/// A condition of WHERE, and the matched rows left once it and those
/// before it hold.
#[derive(Clone, Debug, Serialize)]
pub struct Condition {
    pub text: String,
    pub left: Option<u64>,
}

/// The rows of a query's funnel ([`Probes::finish`]).
#[derive(Clone, Debug, Serialize)]
pub struct Rows {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The table's live rows.
    pub live: Option<u64>,
    /// The rows any input matches.
    pub matched: Option<u64>,
    /// Those WHERE keeps.
    pub selected: Option<u64>,
    /// Those the ranking scores: each input's top of the selected rows.
    pub candidates: Option<u64>,
    /// The rows ranked up to the page's end (LIMIT + OFFSET), and on the
    /// page.
    pub ranked: u64,
    pub shown: u64,
    pub offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fused: Option<String>,
    pub inputs: Vec<InputRows>,
    pub conditions: Vec<Condition>,
    pub phases: Vec<Phase>,
    pub rows: Vec<Row>,
    pub notes: Vec<String>,
}

/// A rerank phase: what it ranks by, and the rows it takes.
#[derive(Clone, Debug, Serialize)]
pub struct Phase {
    pub text: String,
    pub count: usize,
}

/// The value as a number, for a score.
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Float(x) => Some(*x),
        Value::Int(x) => Some(*x as f64),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

fn key_json(v: &Value) -> serde_json::Value {
    crate::query::blend::value_json(v)
}

/// A key's identity among the rows seen.
fn key_id(v: &Value) -> String {
    key_json(v).to_string()
}

/// The conditions of a filter, one by one: the items of a top `AND`.
fn conjuncts(p: &Pred) -> Vec<Pred> {
    match p {
        Pred::True => Vec::new(),
        Pred::And(items) => items.iter().flat_map(conjuncts).collect(),
        p => vec![p.clone()],
    }
}

/// `p` as SQL writes it, as `EXPLAIN` shows a filter.
pub fn pred_text(schema: &Schema, p: &Pred, q: &Select) -> String {
    let probe = Select { filter: p.clone(), ..count_of(q, Pred::True) };
    let plan = crate::query::ir::Plan::of_select(schema, &probe);
    plan.explain(&[schema]).iter().find_map(|l| l.trim_start().strip_prefix("filter: ").map(str::to_string)).unwrap_or_else(|| format!("{p:?}"))
}

/// A query that counts the rows of `q`'s table `filter` keeps.
fn count_of(q: &Select, filter: Pred) -> Select {
    Select {
        projection: vec![("n".into(), Projection::Count)],
        filter,
        order: None,
        group_by: Vec::new(),
        having: None,
        tiebreak: None,
        phases: Vec::new(),
        limit: None,
        offset: 0,
        types: vec![None],
        guard: None,
        per: None,
        ..q.clone()
    }
}

/// A query of the keys of `q`'s table, and of `score` if any, `filter`
/// keeping them, ordered by `order`, at most `limit`.
fn keys_of(q: &Select, key: usize, score: Option<Projection>, filter: Pred, order: Option<OrderBy>, limit: Option<usize>) -> Select {
    let mut projection = vec![("key".to_string(), Projection::Field(key))];
    if let Some(s) = score {
        projection.push(("s".into(), s));
    }
    let n = projection.len();
    Select { projection, filter, order, group_by: Vec::new(), having: None, phases: Vec::new(), limit, offset: 0, types: vec![None; n], guard: None, ..q.clone() }
}

/// The rows a leaf score matches, as a condition, `None` where it has no
/// count of its own.
fn matches_of(schema: &Schema, key: Option<usize>, s: &Score) -> Option<Pred> {
    Some(match s {
        Score::Bm25 { field, text, opts } => {
            let expand = crate::query::logical::Expand::from_opts(opts).ok()?;
            let need = opts.get("min").and_then(|v| v.as_i64()).filter(|&n| n > 1).unwrap_or(1);
            Pred::Match { field: *field, query: format!("({text})@{need}"), expand }
        }
        Score::Similarity { field, .. } | Score::MaxSim { field, .. } | Score::Sparse { field, .. } | Score::Column { field } | Score::Decay { field, .. } => {
            let f = schema.fields.get(*field)?;
            // A column's every row with a value; a vector's every row with
            // one is as near as it is, and has no count of its own.
            match f.ty {
                FieldType::Int | FieldType::Float | FieldType::Keyword => Pred::Present { field: *field },
                _ => return None,
            }
        }
        Score::Ranked { keys } => Pred::In { field: key?, values: keys.clone() },
        _ => return None,
    })
}

fn kind_of(s: &Score) -> &'static str {
    match s {
        Score::Bm25 { .. } => "bm25",
        Score::Similarity { .. } => "vector",
        Score::Sparse { .. } => "sparse",
        Score::MaxSim { .. } => "maxsim",
        Score::Column { .. } => "column",
        Score::Decay { .. } => "decay",
        Score::Ppr { .. } | Score::Hops { .. } => "graph",
        Score::Ranked { .. } => "ranked",
        _ => "score",
    }
}

fn field_of(schema: &Schema, s: &Score) -> Option<String> {
    let f = match s {
        Score::Bm25 { field, .. } | Score::Similarity { field, .. } | Score::Sparse { field, .. } | Score::MaxSim { field, .. } | Score::Column { field } | Score::Decay { field, .. } => *field,
        _ => return None,
    };
    schema.fields.get(f).map(|f| f.name.clone())
}

impl Probes {
    /// The probes of bound query `q` of `schema`'s table, ranked inputs
    /// taking their top `fusion_depth` rows where nothing says.
    pub fn of(schema: &Schema, q: &Select, fusion_depth: usize) -> Probes {
        let key = schema.primary_key;
        let key_name = key.and_then(|k| schema.fields.get(k)).map(|f| f.name.clone());
        let reach = q.limit.unwrap_or(crate::query::exec::DEFAULT_K).saturating_add(q.offset);
        let grouped = !q.group_by.is_empty() || q.projection.iter().any(|(_, p)| matches!(p, Projection::Count | Projection::Agg { .. }));
        let mut conds = conjuncts(&q.filter);
        if conds.len() > MAX_CONDITIONS {
            let rest = conds.split_off(MAX_CONDITIONS - 1);
            conds.push(Pred::And(rest));
        }
        let cond_text = conds.iter().map(|c| pred_text(schema, c, q)).collect();
        let phases = q.phases.iter().map(|p| (crate::query::rank::Rank::of_phase(&p.score).explain(schema), p.count)).collect::<Vec<_>>();
        let mut p = Probes { kind: "other", q: q.clone(), key, key_name, inputs: Vec::new(), fused: None, conds, cond_text, reach, ranked_n: reach, phases };
        if grouped || key.is_none() {
            return p;
        }
        match &q.order {
            Some(OrderBy::Score { score, desc, .. }) => {
                let leaf = |s: &Score, generates: bool, depth: usize| Input {
                    score: s.clone(),
                    label: crate::query::rank::leaf_name(s, schema),
                    kind: kind_of(s),
                    generates,
                    depth,
                    matches: matches_of(schema, key, s),
                    text: match s {
                        Score::Bm25 { text, .. } => Some(text.clone()),
                        _ => None,
                    },
                    field: field_of(schema, s),
                };
                match Rank::of(score, fusion_depth) {
                    Some(rank) => {
                        let depth = rank.depth.max(reach);
                        p.fused = Some(rank.explain(schema));
                        p.inputs = rank.leaves.iter().zip(&rank.generates).take(MAX_INPUTS).map(|(s, &g)| leaf(s, g && !rank.exact, depth)).collect();
                        // The ranking past the page, to the candidates' depth:
                        // a larger LIMIT would not change who is a candidate.
                        p.ranked_n = depth.max(reach).min(MAX_TOP.max(reach));
                        p.kind = "search";
                    }
                    None if score.is_leaf() => {
                        // One input ranks every row WHERE keeps: the page is
                        // its top.
                        let _ = desc;
                        p.inputs = vec![leaf(score, score.is_relevance(), reach)];
                        p.ranked_n = reach + PAST;
                        p.kind = if score.is_relevance() { "search" } else { "rows" };
                    }
                    None => {
                        p.ranked_n = reach + PAST;
                        p.kind = "rows";
                    }
                }
            }
            None | Some(OrderBy::Path { .. }) => {
                p.ranked_n = reach + PAST;
                p.kind = "rows";
            }
            _ => {}
        }
        if !q.phases.is_empty() {
            p.ranked_n = p.ranked_n.max(q.phases[0].count.min(MAX_TOP));
        }
        p
    }

    /// The rows any input that has candidates matches, as a condition:
    /// every row when one of them has no count of its own (the nearest
    /// vectors, which rank every row that has one).
    fn matched_any(&self) -> Pred {
        let gen: Vec<&Input> = self.inputs.iter().filter(|i| i.generates).collect();
        if gen.is_empty() || gen.iter().any(|i| i.matches.is_none()) {
            return Pred::True;
        }
        let mut ms: Vec<Pred> = gen.iter().filter_map(|i| i.matches.clone()).collect();
        if ms.len() == 1 {
            return ms.pop().unwrap_or(Pred::True);
        }
        Pred::Or(ms)
    }

    /// The first round of probes.
    pub fn first(&self) -> Vec<Probe> {
        let Some(key) = self.key else { return Vec::new() };
        if self.kind == "other" {
            return Vec::new();
        }
        let q = &self.q;
        let mut out = vec![Probe { name: "live".into(), select: count_of(q, Pred::True) }];
        // The ranking, past the page: the first phase's order where there
        // are rerank phases; then the page as the query answers it.
        let first_phase = Select { phases: Vec::new(), ..q.clone() };
        let score = matches!(q.order, Some(OrderBy::Score { .. })).then_some(Projection::Score);
        out.push(Probe { name: "ranked".into(), select: keys_of(&first_phase, key, score.clone(), q.filter.clone(), q.order.clone(), Some(self.ranked_n)) });
        let mut page = keys_of(q, key, score, q.filter.clone(), q.order.clone(), q.limit.or(Some(crate::query::exec::DEFAULT_K)));
        (page.offset, page.phases) = (q.offset, q.phases.clone());
        out.push(Probe { name: "page".into(), select: page });
        for (i, inp) in self.inputs.iter().enumerate() {
            if let Some(m) = &inp.matches {
                out.push(Probe { name: format!("matched {i}"), select: count_of(q, m.clone()) });
            }
            if !inp.generates {
                continue;
            }
            // Past its depth, to show what was just below it, and without
            // WHERE, what WHERE took from its top.
            let order = Some(OrderBy::Score { score: inp.score.clone(), desc: true, nulls_first: false });
            let n = (inp.depth + PAST).min(MAX_TOP.max(inp.depth));
            out.push(Probe { name: format!("top {i}"), select: keys_of(q, key, Some(Projection::Feature(inp.score.clone())), Pred::True, order.clone(), Some(n)) });
            out.push(Probe { name: format!("selected {i}"), select: keys_of(q, key, Some(Projection::Feature(inp.score.clone())), q.filter.clone(), order, Some(n)) });
        }
        let m = self.matched_any();
        out.push(Probe { name: "cond 0".into(), select: count_of(q, m.clone()) });
        for j in 0..self.conds.len() {
            let mut all = vec![m.clone()];
            all.extend(self.conds[..=j].iter().cloned());
            out.push(Probe { name: format!("cond {}", j + 1), select: count_of(q, Pred::And(all).simplify()) });
        }
        out
    }

    /// The keys the first round saw, each once.
    fn seen(&self, answers: &HashMap<String, ResultSet>) -> Vec<Value> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let names = ["ranked".to_string(), "page".to_string()].into_iter().chain((0..self.inputs.len()).flat_map(|i| [format!("top {i}"), format!("selected {i}")]));
        for n in names {
            if let Some(rs) = answers.get(&n) {
                for r in &rs.rows {
                    if let Some(k) = r.first() {
                        if seen.insert(key_id(k)) {
                            out.push(k.clone());
                        }
                    }
                }
            }
        }
        out
    }

    /// The second round: which of the rows seen pass each condition.
    pub fn second(&self, answers: &HashMap<String, ResultSet>) -> Vec<Probe> {
        let Some(key) = self.key else { return Vec::new() };
        let seen = self.seen(answers);
        if seen.is_empty() || self.conds.is_empty() {
            return Vec::new();
        }
        let n = seen.len();
        self.conds
            .iter()
            .enumerate()
            .map(|(j, c)| {
                let filter = Pred::And(vec![Pred::In { field: key, values: seen.clone() }, c.clone()]).simplify();
                Probe { name: format!("pass {j}"), select: keys_of(&self.q, key, None, filter, None, Some(n)) }
            })
            .collect()
    }

    /// The funnel's rows from the probes' answers (both rounds').
    pub fn finish(&self, answers: &HashMap<String, ResultSet>) -> Rows {
        let count = |name: &str| answers.get(name).and_then(|rs| rs.rows.first()).and_then(|r| r.first()).and_then(num).map(|x| x as u64);
        let list = |name: &str| -> Vec<(Value, Option<f64>)> { answers.get(name).map(|rs| rs.rows.iter().filter_map(|r| Some((r.first()?.clone(), r.get(1).and_then(num)))).collect()).unwrap_or_default() };
        let ni = self.inputs.len();
        let mut rows: Vec<Row> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        let mut row_of = |k: &Value, rows: &mut Vec<Row>| -> usize {
            *at.entry(key_id(k)).or_insert_with(|| {
                rows.push(Row { key: key_json(k), fate: "filtered", stage: Stage::Match, why: String::new(), page: None, rank: None, score: None, matched: vec![None; ni], selected: vec![None; ni], fails: Vec::new() });
                rows.len() - 1
            })
        };
        let mut notes = Vec::new();
        // Each input's top without WHERE, and with it.
        let mut cands: HashSet<String> = HashSet::new();
        let mut inputs = Vec::new();
        for (i, inp) in self.inputs.iter().enumerate() {
            for (pos, (k, s)) in list(&format!("top {i}")).into_iter().enumerate() {
                let r = row_of(&k, &mut rows);
                rows[r].matched[i] = Some((pos + 1, s.unwrap_or(f64::NAN)));
            }
            let sel = list(&format!("selected {i}"));
            let mut n_cands = 0;
            for (pos, (k, _)) in sel.iter().enumerate() {
                let r = row_of(k, &mut rows);
                rows[r].selected[i] = Some(pos + 1);
                if pos < inp.depth {
                    n_cands += 1;
                    cands.insert(key_id(k));
                }
            }
            inputs.push(InputRows {
                label: inp.label.clone(),
                kind: inp.kind,
                field: inp.field.clone(),
                text: inp.text.clone(),
                generates: inp.generates,
                depth: inp.depth,
                matched: count(&format!("matched {i}")),
                candidates: n_cands,
            });
        }
        // The ranking and the page.
        let ranked = list("ranked");
        let page = list("page");
        let page_at: HashMap<String, usize> = page.iter().enumerate().map(|(i, (k, _))| (key_id(k), i + 1)).collect();
        let reach = self.reach;
        let first_phase = self.phases.first().map(|p| p.1);
        for (pos, (k, s)) in ranked.iter().enumerate() {
            let r = row_of(k, &mut rows);
            rows[r].rank = Some(pos + 1);
            rows[r].score = *s;
            if self.inputs.iter().all(|i| !i.generates) {
                cands.insert(key_id(k));
            }
        }
        for (k, s) in &page {
            let r = row_of(k, &mut rows);
            if rows[r].score.is_none() {
                rows[r].score = *s;
            }
        }
        // Which conditions each row seen fails.
        for j in 0..self.conds.len() {
            let Some(rs) = answers.get(&format!("pass {j}")) else { continue };
            let pass: HashSet<String> = rs.rows.iter().filter_map(|r| r.first()).map(key_id).collect();
            for (id, &r) in &at {
                if !pass.contains(id) {
                    rows[r].fails.push(j);
                }
            }
        }
        let passes_known = (0..self.conds.len()).all(|j| answers.contains_key(&format!("pass {j}")));
        for r in rows.iter_mut() {
            let id = r.key.to_string();
            if let Some(&p) = page_at.get(&id) {
                (r.fate, r.stage, r.page) = ("shown", Stage::Fetch, Some(p));
                r.why = format!("on the page, row {p}");
                continue;
            }
            if let Some(rank) = r.rank {
                match first_phase {
                    Some(n) if rank <= n => {
                        (r.fate, r.stage) = ("reranked", Stage::Rerank);
                        r.why = format!("ranked {rank} before the rerank, which placed it past the page");
                    }
                    Some(n) => {
                        (r.fate, r.stage) = ("cut", Stage::Rank);
                        r.why = format!("ranked {rank}, past the {} the rerank takes", rows_word(n));
                    }
                    None if rank <= self.q.offset => {
                        (r.fate, r.stage) = ("offset", Stage::Fetch);
                        r.why = format!("ranked {rank}, before the page (OFFSET {})", self.q.offset);
                    }
                    None => {
                        (r.fate, r.stage) = ("cut", Stage::Rank);
                        r.why = format!("ranked {rank}, past the {} the page reaches", rows_word(reach));
                    }
                }
                continue;
            }
            if !r.fails.is_empty() {
                let first = r.fails[0];
                (r.fate, r.stage) = ("filtered", Stage::Select);
                r.why = format!("WHERE: fails {}", self.cond_text.get(first).cloned().unwrap_or_default());
                continue;
            }
            let id = r.key.to_string();
            if cands.contains(&id) {
                (r.fate, r.stage) = ("cut", Stage::Rank);
                r.why = format!("a candidate, ranked past the {} read", rows_word(self.ranked_n));
            } else if r.selected.iter().any(Option::is_some) || passes_known {
                (r.fate, r.stage) = ("not_candidate", Stage::Select);
                let depths: Vec<String> = self.inputs.iter().filter(|i| i.generates).map(|i| format!("{} {}", i.label, i.depth)).collect();
                r.why = format!("passes WHERE, but below the top rows of every input ({})", depths.join(", "));
            } else {
                (r.fate, r.stage) = ("filtered", Stage::Select);
                r.why = "WHERE dropped it".into();
            }
        }
        let live = count("live");
        let matched = count("cond 0");
        let conditions: Vec<Condition> = self.cond_text.iter().enumerate().map(|(j, t)| Condition { text: t.clone(), left: count(&format!("cond {}", j + 1)) }).collect();
        let selected = match self.conds.len() {
            0 => matched,
            n => count(&format!("cond {n}")),
        };
        let candidates = match self.inputs.iter().any(|i| i.generates) && self.fused.is_some() {
            true => Some(cands.len() as u64),
            false => selected,
        };
        if self.inputs.iter().any(|i| i.generates && i.matches.is_none()) {
            notes.push("a search by meaning matches every row with a vector: its nearest rows are its candidates".into());
        }
        if self.fused.is_some() && candidates.is_some_and(|c| c as usize >= self.ranked_n) {
            notes.push(format!("the ranking shows its first {} candidates", self.ranked_n));
        }
        rows.sort_by(|a, b| {
            let rank_a = (fate_order(a.fate), a.page.or(a.rank).unwrap_or(usize::MAX), best_input(a));
            let rank_b = (fate_order(b.fate), b.page.or(b.rank).unwrap_or(usize::MAX), best_input(b));
            rank_a.cmp(&rank_b)
        });
        Rows {
            kind: self.kind,
            key: self.key_name.clone(),
            live,
            matched,
            selected,
            candidates,
            ranked: ranked.len().min(reach) as u64,
            shown: page.len() as u64,
            offset: self.q.offset,
            fused: self.fused.clone(),
            inputs,
            conditions,
            phases: self.phases.iter().map(|(t, c)| Phase { text: t.clone(), count: *c }).collect(),
            rows,
            notes,
        }
    }
}

/// `1 row`, `2 rows`.
fn rows_word(n: usize) -> String {
    if n == 1 { "1 row".into() } else { format!("{n} rows") }
}

fn fate_order(f: &str) -> u8 {
    match f {
        "shown" => 0,
        "offset" => 1,
        "reranked" => 2,
        "cut" => 3,
        "not_candidate" => 4,
        _ => 5,
    }
}

/// A row's best place in any input's top, for ordering rows that no
/// ranking placed.
fn best_input(r: &Row) -> usize {
    r.matched.iter().flatten().map(|m| m.0).chain(r.selected.iter().flatten().copied()).min().unwrap_or(usize::MAX)
}

/// Runs `probes` with `run` (a database's or a coordinator's query), both
/// rounds, and puts the rows together. A probe that fails leaves its part
/// out, with a note.
pub fn run(probes: &Probes, mut run: impl FnMut(&Select) -> Result<ResultSet>) -> Rows {
    let mut answers = HashMap::new();
    let mut failed = Vec::new();
    for p in probes.first() {
        match run(&p.select) {
            Ok(rs) => {
                answers.insert(p.name, rs);
            }
            Err(e) => failed.push(format!("{}: {e}", p.name)),
        }
    }
    for p in probes.second(&answers) {
        match run(&p.select) {
            Ok(rs) => {
                answers.insert(p.name, rs);
            }
            Err(e) => failed.push(format!("{}: {e}", p.name)),
        }
    }
    let mut rows = probes.finish(&answers);
    rows.notes.extend(failed.into_iter().map(|f| format!("probe {f}")));
    rows
}

// ---------------------------------------------------------------- terms

/// A term a BM25 input searches: a word of its text as analysed, or one
/// its expansions added, with the rows that hold it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Term {
    pub term: String,
    pub df: u64,
    /// `word` (the text's own), `typo` (a neighbour of a word within its
    /// edits), `prefix` (a word a prefix begins), `synonym` (anything
    /// else an expansion added).
    pub kind: &'static str,
    /// The word of the text it stands for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
}

/// The terms of `bm25(field, text)` from the terms its statistics hold
/// (`df`, every term it scores with, its expansions' included) and the
/// field's analyzer: each the text's own word or what an expansion added.
pub fn terms_of(text: &str, analyzer: crate::text::Analyzer, df: &HashMap<String, u64>) -> Vec<Term> {
    let words = analyzer.query_term_seq(text, true);
    let prefixes: Vec<String> = text.split_whitespace().filter_map(|w| w.strip_suffix('*')).map(|w| w.to_lowercase()).collect();
    let mut out: Vec<Term> = Vec::new();
    for w in &words {
        if !out.iter().any(|t| &t.term == w) {
            out.push(Term { term: w.clone(), df: df.get(w).copied().unwrap_or(0), kind: "word", of: None });
        }
    }
    let mut added: Vec<(&String, &u64)> = df.iter().filter(|(t, _)| !words.contains(t)).collect();
    added.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (t, n) in added {
        let tc: Vec<char> = t.chars().collect();
        let near = words.iter().find_map(|w| {
            let e = crate::query::logical::Fuzzy::Auto.edits(w.chars().count()).max(1);
            crate::text::fuzzy::within(&w.chars().collect::<Vec<_>>(), t, e).filter(|&d| d > 0).map(|d| (w, d))
        });
        let (kind, of) = match (prefixes.iter().find(|p| t.starts_with(p.as_str())), near) {
            (Some(p), _) => ("prefix", Some(format!("{p}*"))),
            (None, Some((w, _))) if tc.len() > 1 => ("typo", Some(w.clone())),
            _ => ("synonym", None),
        };
        out.push(Term { term: t.clone(), df: *n, kind, of });
    }
    out
}

/// The answer of a probe that fails for a reason the funnel says.
pub fn refused(why: &str) -> Error {
    Error::Query(format!("funnel: {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(op: &str, depth: u16, start: f64, us: f64) -> Step {
        Step { op: op.into(), depth, start_us: start, us, ..Step::default() }
    }

    #[test]
    fn a_steps_time_counts_once_less_what_runs_inside_it() {
        // rank 100 µs: two leaves 30 + 40 one after the other, the rest its own.
        let steps = vec![step("rank", 0, 0.0, 100.0), step("bm25", 1, 0.0, 30.0), step("segment", 2, 0.0, 30.0), step("vector", 1, 30.0, 40.0), step("project", 0, 100.0, 10.0)];
        let t = step_times(&steps);
        assert_eq!(t[Stage::Match.index()], 70.0);
        assert_eq!(t[Stage::Rank.index()], 30.0);
        assert_eq!(t[Stage::Fetch.index()], 10.0);
        assert_eq!(t.iter().sum::<f64>(), 110.0);
    }

    #[test]
    fn steps_side_by_side_share_their_parents_time() {
        // Two segments on two threads, 60 µs each, in a 70 µs leaf.
        let steps = vec![step("bm25", 0, 0.0, 70.0), step("segment", 1, 5.0, 60.0), step("filter", 2, 5.0, 20.0), step("segment", 1, 5.0, 60.0)];
        let t = step_times(&steps);
        let total: f64 = t.iter().sum();
        assert!((total - 70.0).abs() < 1e-9, "{t:?}");
        // The filter is a sixth of the segments' time: 60 µs of union, 120 of steps.
        assert!((t[Stage::Select.index()] - 10.0).abs() < 1e-9, "{t:?}");
        assert!((t[Stage::Match.index()] - 60.0).abs() < 1e-9, "{t:?}");
    }

    #[test]
    fn spans_add_the_text_stages_and_the_rest_is_other() {
        let steps = vec![step("bm25", 0, 500.0, 100.0)];
        let spans = [
            SpanTime { name: "understand", start_us: 0.0, dur_us: 300.0, call: false },
            // An understanding's own read, inside it: counts for it.
            SpanTime { name: "plan", start_us: 10.0, dur_us: 50.0, call: false },
            SpanTime { name: "plan", start_us: 300.0, dur_us: 150.0, call: false },
            SpanTime { name: "embed", start_us: 320.0, dur_us: 100.0, call: false },
            SpanTime { name: "execute", start_us: 490.0, dur_us: 120.0, call: false },
        ];
        let t = timings(&steps, &spans, 1000.0);
        assert_eq!(t[Stage::Understand.index()], 300.0);
        assert_eq!(t[Stage::Plan.index()], 50.0);
        assert_eq!(t[Stage::Embed.index()], 100.0);
        assert_eq!(t[Stage::Match.index()], 100.0);
        assert_eq!(t[Stage::Other.index()], 450.0);
        let a = aggregate(&[(t, 1000.0), ([0.0; STAGES], 10.0)]);
        let embed = &a.stages[Stage::Embed.index()];
        assert_eq!((embed.ran, embed.p50_ms), (1, 0.1));
        assert_eq!(a.queries, 2);
    }

    #[test]
    fn a_coordinators_calls_count_as_shards_and_the_shards_steps_as_theirs() {
        // Two shard calls side by side, each 100 µs: 20 network, 80 on the shard of which 60 bm25.
        let mut steps = Vec::new();
        for _ in 0..2 {
            steps.extend([step("shard", 0, 0.0, 100.0), step("on the shard", 1, 10.0, 80.0), step("bm25", 2, 15.0, 60.0)]);
        }
        steps.push(step("merge", 0, 100.0, 5.0));
        let t = step_times(&steps);
        assert!((t.iter().sum::<f64>() - 105.0).abs() < 1e-9, "{t:?}");
        assert!((t[Stage::Match.index()] - 60.0).abs() < 1e-9, "{t:?}");
        assert!((t[Stage::Shards.index()] - 40.0).abs() < 1e-9, "{t:?}");
        assert_eq!(t[Stage::Rank.index()], 5.0);
    }

    #[test]
    fn terms_say_what_each_expansion_added() {
        let df: HashMap<String, u64> = [("chair", 40), ("chain", 3), ("seat", 9), ("sofa", 20), ("sofabed", 2)].into_iter().map(|(t, n)| (t.to_string(), n)).collect();
        let t = terms_of("chiar sofa*", crate::text::Analyzer::English, &df);
        let kind = |w: &str| t.iter().find(|x| x.term == w).map(|x| x.kind);
        assert_eq!(kind("chiar"), Some("word"));
        assert_eq!(kind("chair"), Some("typo"));
        assert_eq!(kind("sofabed"), Some("prefix"));
        assert_eq!(kind("seat"), Some("synonym"));
    }
}
