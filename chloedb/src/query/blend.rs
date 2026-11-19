//! Blends: one list made from the rows of several queries, after each has
//! ranked its own.
//!
//! ```sql
//! CREATE BLEND serp AS
//!   SOURCES (
//!     stories  AS (SELECT id, title, author, emb, score() AS relevance, ln(1 + points) AS quality
//!                  FROM items WHERE kind = 'story' ORDER BY bm25(title, :q) DESC LIMIT 100),
//!     comments AS (SELECT id, text AS title, author, emb, score() AS relevance, ln(1 + replies) AS quality
//!                  FROM comments ORDER BY bm25(text, :q) DESC LIMIT 60)
//!   )
//!   HEADS (relevance CALIBRATE tmm, quality CALIBRATE minmax MISSING zero)
//!   UTILITY 1 * relevance + 0.2 * quality
//!   SLOT 1 => source = 'stories'
//!   QUOTA (source = 'comments' BETWEEN 20% AND 40% IN TOP 10)
//!   DIVERSIFY 0.3 ON emb
//!   MAX 2 PER author
//!   DEDUPE 0.95 ON emb
//!   WITH (k => 10);
//!
//! SELECT slot, source, id, title, utility, blend_decision FROM blend('serp', q => :q);
//! SELECT * FROM blend('serp@3', q => 'rust async') LIMIT 20;   -- version 3
//! DROP BLEND serp;
//! ```
//!
//! * **Sources** are queries, each ranked as any query is (a rank
//!   profile, `rrf`, `recommend(...)`), run with the call's arguments as
//!   their parameters. A row's key is its table's key (the first column
//!   where there is none): rows of one key from two sources are one row,
//!   kept where its utility is higher.
//! * **Heads** are the values a row is worth by: columns the sources name
//!   alike (`... AS relevance`), and `score`, each source's own ranking
//!   score, without a column. Each head is calibrated within its source
//!   (`tmm`: the source's best row is 1; `minmax`; `rank`: only the order
//!   counts; `raw`), and a source that lacks a head gives its rows the
//!   mean of the others' (`MISSING neutral`) or 0 (`MISSING zero`).
//! * **Utility** is the weighted sum of the calibrated heads, plus each
//!   source's `BOOST`.
//! * **The fill** places rows slot by slot, best utility first, under the
//!   rules: `SLOT n` keeps a slot for rows of a kind, `QUOTA` holds a kind
//!   between shares of the top n (a quota that can only be met by placing
//!   its kind now places it), `SPACING` keeps a kind at most n in any m
//!   rows in a row, `MAX n PER column` caps rows of one value, `DIVERSIFY
//!   λ ON column` takes λ of the utilities' range off a row for its
//!   likeness to a row placed (cosine on a vector, equality otherwise),
//!   and `DEDUPE t ON vector` drops a row whose cosine to a row placed is
//!   t or more. When no row fits every rule the fill sets one aside, and
//!   says which.
//! * **Receipts.** Each row says why it is where it is (`blend_decision`);
//!   [`Blended::receipts`] adds the rows passed over and why, each quota's
//!   count, and each rule's cost: the share of the slate's discounted
//!   utility it gives up.
//! * **Kept as a function.** `CREATE BLEND` writes a stored function
//!   `name()` whose body is the definition ([`MARK`] and JSON), so a blend
//!   is versioned (`blend('name@3')`), kept by checkpoints and copies,
//!   shipped to replicas and to a coordinator's shards as functions are,
//!   and no file format changes.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr};
use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::error::{Error, Result};
use crate::query::exec::ResultSet;
use crate::query::function::{Function, FunctionSet};
use crate::query::Params;
use crate::value::Value;

/// The table function that blends.
pub const NAME: &str = "blend";
/// How a blend's function body begins.
pub const MARK: &str = "blend:";
/// How `CREATE BLEND` is written.
pub const CREATE_FORM: &str = "CREATE [OR REPLACE] BLEND name AS SOURCES (name AS (SELECT ...), ...) [HEADS (head [CALIBRATE tmm|minmax|rank|raw] [LO n] [MISSING neutral|zero], ...)] [UTILITY w * head + ...] [BOOST (source w, ...)] [SLOT n => column = 'value'] [QUOTA (column = 'value' BETWEEN a% AND b% | AT LEAST a% [IF ANY] | AT MOST b% [IN TOP n], ...)] [SPACING (column = 'value' AT MOST n PER m ROWS, ...)] [DIVERSIFY λ ON column] [MAX n PER column] [DEDUPE t ON vector_column] [WITH (k => 10, note => '...')]";
/// How `DROP BLEND` is written.
pub const DROP_FORM: &str = "DROP BLEND [IF EXISTS] name";
/// How a call is written.
pub const USAGE: &str = "SELECT ... FROM blend('name[@version]' [, parameter => value, ...]) [LIMIT k]";
/// Sources a blend takes at most.
pub const MAX_SOURCES: usize = 16;
/// Rows a blend places at most.
pub const MAX_K: usize = 1000;
/// The columns every blended row has, besides its source's.
pub const COLUMNS: [&str; 5] = ["slot", "source", "key", "utility", "blend_decision"];
/// The rank constant of `CALIBRATE rank`, as `rrf`'s.
const RRF_K: f64 = 60.0;
/// Rows passed over that the receipts list at most.
const PASSED: usize = 20;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

fn default_k() -> usize {
    10
}

fn one() -> f64 {
    1.0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A blend's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Def {
    /// Lower case.
    pub name: String,
    pub sources: Vec<Source>,
    /// None: one head, `score`, each source's own score by `tmm`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heads: Vec<Head>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boost: Vec<Boost>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quotas: Vec<Quota>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spacing: Vec<Spacing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diversify: Option<Diversify>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<Cap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe: Option<Dedupe>,
    /// Rows placed when the call has no LIMIT.
    #[serde(default = "default_k")]
    pub k: usize,
    /// What this version changed, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A query whose rows a blend places.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Lower case; the value of the column `source`.
    pub name: String,
    pub sql: String,
}

/// A value rows are worth by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Head {
    /// Lower case: the column of each source's rows it reads (`score`
    /// without one: the source's own score).
    pub name: String,
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default)]
    pub calibrate: Calibrate,
    /// The least value `tmm` scales from (0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lo: Option<f64>,
    #[serde(default)]
    pub missing: Missing,
}

/// How a head's values of one source are put on one scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Calibrate {
    /// (x − lo) / (the source's best − lo): its best row is 1.
    #[default]
    Tmm,
    /// (x − min) / (max − min) over the source's rows.
    Minmax,
    /// (60 + 1) / (60 + r), r the row's rank by the head in its source.
    Rank,
    /// The value as the source computed it.
    Raw,
}

impl Calibrate {
    pub fn name(self) -> &'static str {
        match self {
            Calibrate::Tmm => "tmm",
            Calibrate::Minmax => "minmax",
            Calibrate::Rank => "rank",
            Calibrate::Raw => "raw",
        }
    }
}

/// What a row whose source lacks a head is worth on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Missing {
    /// The mean of the calibrated values the other rows have.
    #[default]
    Neutral,
    Zero,
}

/// Utility added to every row of a source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Boost {
    pub source: String,
    pub by: f64,
}

/// Rows of a kind: a column (`source`, or a column of the rows) equal to
/// a value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pred {
    pub column: String,
    pub value: Json,
}

impl Pred {
    pub fn sql(&self) -> String {
        format!("{} = {}", crate::query::sql::ident(&self.column), json_sql(&self.value))
    }

    fn same(&self, o: &Pred) -> bool {
        self.column.eq_ignore_ascii_case(&o.column) && json_text(&self.value) == json_text(&o.value)
    }
}

/// `SLOT n => pred`: slot n is kept for rows of a kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub slot: usize,
    pub when: Pred,
}

/// `QUOTA (pred BETWEEN min% AND max% IN TOP n)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    pub when: Pred,
    /// Percent, 0 to 100.
    #[serde(default)]
    pub min: f64,
    #[serde(default = "hundred")]
    pub max: f64,
    /// Over the top n rows (all the rows placed, without).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<usize>,
    /// The quota holds only when some row is of its kind.
    #[serde(default, skip_serializing_if = "is_false")]
    pub if_any: bool,
}

fn hundred() -> f64 {
    100.0
}

impl Quota {
    pub fn sql(&self) -> String {
        let mut s = self.when.sql();
        match (self.min > 0.0, self.max < 100.0) {
            (true, true) => s += &format!(" BETWEEN {}% AND {}%", num(self.min), num(self.max)),
            (true, false) => s += &format!(" AT LEAST {}%", num(self.min)),
            _ => s += &format!(" AT MOST {}%", num(self.max)),
        }
        if let Some(n) = self.top {
            s += &format!(" IN TOP {n}");
        }
        if self.if_any {
            s += " IF ANY";
        }
        s
    }

    /// The rows of the top it covers, of `k` placed.
    fn over(&self, k: usize) -> usize {
        self.top.unwrap_or(k).min(k)
    }

    /// The rows of its kind it needs, and allows, in its top.
    fn bounds(&self, k: usize) -> (usize, usize) {
        let n = self.over(k) as f64;
        ((self.min / 100.0 * n - 1e-9).ceil().max(0.0) as usize, (self.max / 100.0 * n + 1e-9).floor() as usize)
    }
}

/// `SPACING (pred AT MOST n PER m ROWS)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spacing {
    pub when: Pred,
    pub at_most: usize,
    pub per: usize,
}

impl Spacing {
    pub fn sql(&self) -> String {
        format!("{} AT MOST {} PER {} ROWS", self.when.sql(), self.at_most, self.per)
    }
}

/// `DIVERSIFY λ ON column`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Diversify {
    pub lambda: f64,
    pub on: String,
}

/// `MAX n PER column`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cap {
    pub at_most: usize,
    pub per: String,
}

/// `DEDUPE t ON vector_column`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dedupe {
    pub threshold: f64,
    pub on: String,
}

/// A rule of the fill, as [`Blended::receipts`] prices it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Slot(usize),
    Quota(usize),
    Spacing(usize),
    Cap(usize),
    Diversify,
    Dedupe,
}

impl Def {
    /// The heads it reads: its own, or `score` by `tmm`.
    pub fn heads(&self) -> Vec<Head> {
        if self.heads.is_empty() {
            return vec![Head { name: "score".into(), weight: 1.0, calibrate: Calibrate::Tmm, lo: None, missing: Missing::Neutral }];
        }
        self.heads.clone()
    }

    /// The definition a stored function's body holds, if it is one.
    pub fn of_body(body: &str) -> Option<Def> {
        let s = body.trim();
        let s = s.strip_prefix('\'')?.strip_suffix('\'')?.replace("''", "'");
        serde_json::from_str(s.strip_prefix(MARK)?).ok()
    }

    /// The statement that keeps it as a function.
    pub fn function_sql(&self, replace: bool) -> String {
        let body = format!("{MARK}{}", serde_json::to_string(self).expect("a definition"));
        let names: Vec<&str> = self.sources.iter().map(|s| s.name.as_str()).collect();
        let what = match &self.note {
            Some(n) if !n.trim().is_empty() => format!("blend of {}: {}", names.join(", "), n.trim()),
            _ => format!("blend of {}", names.join(", ")),
        };
        format!("CREATE {}FUNCTION {}() RETURNS TEXT OPTIONS (description = {}) AS {}", if replace { "OR REPLACE " } else { "" }, self.name, quote(&what), quote(&body))
    }

    /// The source named `name`.
    pub fn source(&self, name: &str) -> Option<usize> {
        self.sources.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    fn rules(&self) -> Vec<Rule> {
        let mut r: Vec<Rule> = (0..self.slots.len()).map(Rule::Slot).collect();
        r.extend((0..self.quotas.len()).map(Rule::Quota));
        r.extend((0..self.spacing.len()).map(Rule::Spacing));
        r.extend((0..self.caps.len()).map(Rule::Cap));
        if self.diversify.as_ref().is_some_and(|d| d.lambda > 0.0) {
            r.push(Rule::Diversify);
        }
        if self.dedupe.is_some() {
            r.push(Rule::Dedupe);
        }
        r
    }

    fn rule_sql(&self, r: Rule) -> String {
        match r {
            Rule::Slot(i) => format!("SLOT {} => {}", self.slots[i].slot, self.slots[i].when.sql()),
            Rule::Quota(i) => format!("QUOTA {}", self.quotas[i].sql()),
            Rule::Spacing(i) => format!("SPACING {}", self.spacing[i].sql()),
            Rule::Cap(i) => format!("MAX {} PER {}", self.caps[i].at_most, crate::query::sql::ident(&self.caps[i].per)),
            Rule::Diversify => self.diversify.as_ref().map_or(String::new(), |d| format!("DIVERSIFY {} ON {}", num(d.lambda), crate::query::sql::ident(&d.on))),
            Rule::Dedupe => self.dedupe.as_ref().map_or(String::new(), |d| format!("DEDUPE {} ON {}", num(d.threshold), crate::query::sql::ident(&d.on))),
        }
    }

    /// `CREATE [OR REPLACE] BLEND` that defines it, as it reads back.
    pub fn sql(&self, replace: bool) -> String {
        use crate::query::sql::ident;
        let mut s = format!("CREATE {}BLEND {} AS\n  SOURCES (\n", if replace { "OR REPLACE " } else { "" }, ident(&self.name));
        for (i, src) in self.sources.iter().enumerate() {
            s += &format!("    {} AS ({}){}\n", ident(&src.name), src.sql.trim(), if i + 1 < self.sources.len() { "," } else { "" });
        }
        s += "  )";
        if !self.heads.is_empty() {
            let hs: Vec<String> = self
                .heads
                .iter()
                .map(|h| {
                    let mut t = format!("{} CALIBRATE {}", ident(&h.name), h.calibrate.name());
                    if let Some(lo) = h.lo {
                        t += &format!(" LO {}", num(lo));
                    }
                    if h.missing == Missing::Zero {
                        t += " MISSING zero";
                    }
                    t
                })
                .collect();
            s += &format!("\n  HEADS ({})", hs.join(", "));
            let mut u = String::new();
            for (i, h) in self.heads.iter().enumerate() {
                match (i, h.weight < 0.0) {
                    (0, _) => u += &format!("{} * {}", num(h.weight), ident(&h.name)),
                    (_, false) => u += &format!(" + {} * {}", num(h.weight), ident(&h.name)),
                    (_, true) => u += &format!(" - {} * {}", num(-h.weight), ident(&h.name)),
                }
            }
            s += &format!("\n  UTILITY {u}");
        }
        if !self.boost.is_empty() {
            let bs: Vec<String> = self.boost.iter().map(|b| format!("{} {}", ident(&b.source), num(b.by))).collect();
            s += &format!("\n  BOOST ({})", bs.join(", "));
        }
        for sl in &self.slots {
            s += &format!("\n  SLOT {} => {}", sl.slot, sl.when.sql());
        }
        if !self.quotas.is_empty() {
            let qs: Vec<String> = self.quotas.iter().map(Quota::sql).collect();
            s += &format!("\n  QUOTA ({})", qs.join(",\n         "));
        }
        if !self.spacing.is_empty() {
            let ss: Vec<String> = self.spacing.iter().map(Spacing::sql).collect();
            s += &format!("\n  SPACING ({})", ss.join(", "));
        }
        for r in [Rule::Diversify].into_iter().filter(|_| self.diversify.is_some()) {
            s += &format!("\n  {}", self.rule_sql(r));
        }
        for i in 0..self.caps.len() {
            s += &format!("\n  {}", self.rule_sql(Rule::Cap(i)));
        }
        if self.dedupe.is_some() {
            s += &format!("\n  {}", self.rule_sql(Rule::Dedupe));
        }
        s += &format!("\n  WITH (k => {}", self.k);
        if let Some(n) = self.note.as_deref().filter(|n| !n.trim().is_empty()) {
            s += &format!(", note => {}", quote(n));
        }
        s += ")";
        s
    }

    /// Checks what a definition says of itself: its names, numbers and
    /// rules, each source a query, and quotas the slots can hold.
    pub fn check(&self) -> Result<()> {
        let plain = |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !n.starts_with(|c: char| c.is_ascii_digit());
        if !plain(&self.name) {
            return Err(err(format!("a blend's name is letters, digits and _, not {:?}", self.name)));
        }
        if self.sources.is_empty() || self.sources.len() > MAX_SOURCES {
            return Err(err(format!("a blend has 1 to {MAX_SOURCES} sources, not {}", self.sources.len())));
        }
        for (i, s) in self.sources.iter().enumerate() {
            if !plain(&s.name) {
                return Err(err(format!("a source's name is letters, digits and _, not {:?}", s.name)));
            }
            if self.sources[..i].iter().any(|o| o.name.eq_ignore_ascii_case(&s.name)) {
                return Err(err(format!("two sources are named {}", s.name)));
            }
            let q = crate::query::sql::parse_query(&s.sql).map_err(|e| err(format!("source {}: {e}", s.name)))?;
            if call_name(&q).is_some() {
                return Err(err(format!("source {} reads blend(...): a blend's sources are queries of tables", s.name)));
            }
        }
        if !(1..=MAX_K).contains(&self.k) {
            return Err(err(format!("k is 1 to {MAX_K}, not {}", self.k)));
        }
        for (i, h) in self.heads.iter().enumerate() {
            if !plain(&h.name) {
                return Err(err(format!("a head's name is a column's, not {:?}", h.name)));
            }
            if self.heads[..i].iter().any(|o| o.name.eq_ignore_ascii_case(&h.name)) {
                return Err(err(format!("head {} is named twice", h.name)));
            }
            if !h.weight.is_finite() || h.lo.is_some_and(|x| !x.is_finite()) {
                return Err(err(format!("head {}'s weight and LO are numbers", h.name)));
            }
        }
        for b in &self.boost {
            if self.source(&b.source).is_none() {
                return Err(err(format!("BOOST names {}, which is not a source", b.source)));
            }
            if !b.by.is_finite() {
                return Err(err("a BOOST is a number"));
            }
        }
        let pred = |p: &Pred, what: &str| -> Result<()> {
            if p.column.is_empty() {
                return Err(err(format!("{what} names no column")));
            }
            if p.column.eq_ignore_ascii_case("source") && self.source(&json_text(&p.value)).is_none() {
                return Err(err(format!("{what} names source {}, which the blend does not have", json_text(&p.value))));
            }
            Ok(())
        };
        for (i, s) in self.slots.iter().enumerate() {
            if !(1..=MAX_K).contains(&s.slot) {
                return Err(err(format!("SLOT {} is 1 to {MAX_K}", s.slot)));
            }
            if self.slots[..i].iter().any(|o| o.slot == s.slot) {
                return Err(err(format!("SLOT {} is kept twice", s.slot)));
            }
            pred(&s.when, &format!("SLOT {}", s.slot))?;
        }
        for q in &self.quotas {
            pred(&q.when, "QUOTA")?;
            if !(0.0..=100.0).contains(&q.min) || !(0.0..=100.0).contains(&q.max) || q.min > q.max {
                return Err(err(format!("QUOTA {}: the shares are percents, the least no more than the most", q.sql())));
            }
            if q.top == Some(0) {
                return Err(err("QUOTA ... IN TOP n takes n of 1 or more"));
            }
        }
        for s in &self.spacing {
            pred(&s.when, "SPACING")?;
            if s.per < 2 || s.at_most >= s.per {
                return Err(err(format!("SPACING {}: at most n in m rows, n less than m and m 2 or more", s.sql())));
            }
        }
        if let Some(d) = &self.diversify {
            if !(0.0..=1.0).contains(&d.lambda) || d.on.is_empty() {
                return Err(err("DIVERSIFY λ ON column takes λ from 0 to 1"));
            }
        }
        for c in &self.caps {
            if c.at_most == 0 || c.per.is_empty() {
                return Err(err("MAX n PER column takes n of 1 or more"));
            }
        }
        if let Some(d) = &self.dedupe {
            if !(d.threshold > 0.0 && d.threshold <= 1.0) || d.on.is_empty() {
                return Err(err("DEDUPE t ON column takes a cosine t above 0, at most 1"));
            }
        }
        if self.note.as_ref().is_some_and(|n| n.len() > 500) {
            return Err(err("a note is at most 500 bytes"));
        }
        self.feasible(self.k)
    }

    /// Whether the slots can hold every quota's least share, and every
    /// pinned slot fits the quotas' most.
    pub fn feasible(&self, k: usize) -> Result<()> {
        for q in self.quotas.iter().filter(|q| q.min > 0.0) {
            let n = q.over(k);
            // Each kind on the quota's column: the most any quota of it
            // within these n rows needs, or the slots kept for it.
            let mut need: Vec<(&Pred, usize)> = Vec::new();
            for p in self.quotas.iter().filter(|p| p.when.column.eq_ignore_ascii_case(&q.when.column) && p.over(k) <= n) {
                let want = p.bounds(k).0;
                match need.iter_mut().find(|(w, _)| w.same(&p.when)) {
                    Some((_, m)) => *m = (*m).max(want),
                    None => need.push((&p.when, want)),
                }
            }
            let mut demand = 0;
            for (w, m) in &need {
                demand += (*m).max(self.slots.iter().filter(|s| s.slot <= n && s.when.same(w)).count());
            }
            demand += self.slots.iter().filter(|s| s.slot <= n && s.when.column.eq_ignore_ascii_case(&q.when.column) && !need.iter().any(|(w, _)| s.when.same(w))).count();
            if demand > n {
                return Err(err(format!("the quotas on {} and its slots ask for {demand} of the top {n} rows", q.when.column)));
            }
        }
        for q in &self.quotas {
            let n = q.over(k);
            let pinned = self.slots.iter().filter(|s| s.slot <= n && s.when.same(&q.when)).count();
            if pinned > q.bounds(k).1 {
                return Err(err(format!("{pinned} slots are kept for {} but QUOTA {} allows {}", q.when.sql(), q.sql(), q.bounds(k).1)));
            }
        }
        Ok(())
    }
}

/// `CREATE BLEND` or `DROP BLEND`.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Create { def: Def, replace: bool },
    Drop { name: String, if_exists: bool },
}

/// A token, and where it starts in the text.
struct Toks<'a> {
    text: &'a str,
    toks: Vec<(Token, usize)>,
    i: usize,
}

impl<'a> Toks<'a> {
    fn of(text: &'a str) -> Option<Toks<'a>> {
        let all = Tokenizer::new(&GenericDialect {}, text).tokenize_with_location().ok()?;
        let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let at = |l: &sqlparser::tokenizer::Location| -> usize {
            let Some(&ls) = starts.get((l.line as usize).saturating_sub(1)) else { return text.len() };
            text[ls..].char_indices().nth((l.column as usize).saturating_sub(1)).map_or(text.len(), |(b, _)| ls + b)
        };
        let mut toks: Vec<(Token, usize)> = all.into_iter().filter(|t| !matches!(t.token, Token::Whitespace(_))).map(|t| { let o = at(&t.span.start); (t.token, o) }).collect();
        if toks.last().is_some_and(|t| t.0 == Token::SemiColon) {
            toks.pop();
        }
        Some(Toks { text, toks, i: 0 })
    }

    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.i).map(|t| &t.0)
    }

    fn word(&self) -> String {
        match self.peek() {
            Some(Token::Word(w)) if w.quote_style.is_none() => w.value.to_ascii_uppercase(),
            _ => String::new(),
        }
    }

    fn at_word(&self, w: &str) -> bool {
        self.word() == w
    }

    fn eat_word(&mut self, w: &str) -> bool {
        let y = self.at_word(w);
        if y {
            self.i += 1;
        }
        y
    }

    fn eat(&mut self, t: &Token) -> bool {
        let y = self.peek() == Some(t);
        if y {
            self.i += 1;
        }
        y
    }

    fn next_text(&self) -> String {
        self.peek().map_or("the end".into(), |t| t.to_string())
    }

    fn want_word(&mut self, w: &str, what: &str) -> Result<()> {
        if self.eat_word(w) {
            return Ok(());
        }
        Err(err(format!("expected {what}, not {}; the form is {CREATE_FORM}", self.next_text())))
    }

    fn want(&mut self, t: Token, what: &str) -> Result<()> {
        if self.eat(&t) {
            return Ok(());
        }
        Err(err(format!("expected {what}, not {}; the form is {CREATE_FORM}", self.next_text())))
    }

    /// A name: lower case unless quoted.
    fn name(&mut self, what: &str) -> Result<String> {
        match self.peek() {
            Some(Token::Word(w)) => {
                let n = if w.quote_style.is_none() { w.value.to_ascii_lowercase() } else { w.value.clone() };
                self.i += 1;
                Ok(n)
            }
            _ => Err(err(format!("expected {what}, not {}", self.next_text()))),
        }
    }

    /// A column, as written.
    fn column(&mut self, what: &str) -> Result<String> {
        match self.peek() {
            Some(Token::Word(w)) => {
                let n = w.value.clone();
                self.i += 1;
                Ok(n)
            }
            _ => Err(err(format!("expected {what}, not {}", self.next_text()))),
        }
    }

    fn number(&mut self, what: &str) -> Result<f64> {
        let neg = self.eat(&Token::Minus);
        match self.peek() {
            Some(Token::Number(n, _)) => {
                let x: f64 = n.parse().map_err(|_| err(format!("{n} is not a number")))?;
                self.i += 1;
                Ok(if neg { -x } else { x })
            }
            _ => Err(err(format!("expected {what}, a number, not {}", self.next_text()))),
        }
    }

    fn whole(&mut self, what: &str) -> Result<usize> {
        let x = self.number(what)?;
        if x < 0.0 || x.fract() != 0.0 || x > 1e9 {
            return Err(err(format!("{what} is a whole number, not {x}")));
        }
        Ok(x as usize)
    }

    fn percent(&mut self, what: &str) -> Result<f64> {
        let x = self.number(what)?;
        self.eat(&Token::Mod);
        Ok(x)
    }

    fn value(&mut self) -> Result<Json> {
        let neg = matches!(self.peek(), Some(Token::Minus));
        match (self.peek().cloned(), neg) {
            (Some(Token::SingleQuotedString(s)), _) => {
                self.i += 1;
                Ok(json!(s))
            }
            (Some(Token::Number(..)), _) | (_, true) => {
                let x = self.number("a value")?;
                Ok(if x.fract() == 0.0 && x.abs() < 9e15 { json!(x as i64) } else { json!(x) })
            }
            (Some(Token::Word(w)), _) if w.quote_style.is_none() && (w.value.eq_ignore_ascii_case("true") || w.value.eq_ignore_ascii_case("false")) => {
                self.i += 1;
                Ok(json!(w.value.eq_ignore_ascii_case("true")))
            }
            _ => Err(err(format!("expected a value ('text', a number, true or false), not {}", self.next_text()))),
        }
    }

    fn pred(&mut self) -> Result<Pred> {
        let column = self.column("a column (source, or a column of the rows)")?;
        self.want(Token::Eq, "= after the column")?;
        Ok(Pred { column, value: self.value()? })
    }

    /// The text inside the parentheses that start here, and moves past
    /// them.
    fn group_text(&mut self, what: &str) -> Result<String> {
        let Some((Token::LParen, open)) = self.toks.get(self.i).cloned() else {
            return Err(err(format!("expected ( before {what}, not {}", self.next_text())));
        };
        let mut depth = 0usize;
        for j in self.i..self.toks.len() {
            match self.toks[j].0 {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        let close = self.toks[j].1;
                        self.i = j + 1;
                        return Ok(self.text[open + 1..close].trim().to_string());
                    }
                }
                _ => {}
            }
        }
        Err(err(format!("{what}: a ( without its )")))
    }

    /// `( item, item, ... )`.
    fn list(&mut self, what: &str, mut item: impl FnMut(&mut Self) -> Result<()>) -> Result<()> {
        self.want(Token::LParen, &format!("( before {what}"))?;
        loop {
            item(self)?;
            if self.eat(&Token::Comma) {
                continue;
            }
            return self.want(Token::RParen, &format!(", or ) in {what}"));
        }
    }
}

/// The statement `text` is, if it is `CREATE BLEND` or `DROP BLEND`.
pub fn parse_statement(text: &str) -> Result<Option<Statement>> {
    let head: String = text.trim_start().chars().take(6).collect::<String>().to_ascii_uppercase();
    if !(head.starts_with("CREATE") || head.starts_with("DROP")) || !text.as_bytes().windows(5).any(|w| w.eq_ignore_ascii_case(b"BLEND")) {
        return Ok(None);
    }
    let Some(mut t) = Toks::of(text) else { return Ok(None) };
    let create = t.eat_word("CREATE");
    if !create && !t.eat_word("DROP") {
        return Ok(None);
    }
    let replace = create && t.at_word("OR") && {
        t.i += 1;
        t.eat_word("REPLACE")
    };
    if !t.eat_word("BLEND") {
        return Ok(None);
    }
    if !create {
        let if_exists = t.at_word("IF") && {
            t.i += 1;
            t.eat_word("EXISTS")
        };
        let name = t.name("the blend's name")?;
        if t.peek().is_some() {
            return Err(err(format!("unexpected {}; the form is {DROP_FORM}", t.next_text())));
        }
        return Ok(Some(Statement::Drop { name, if_exists }));
    }
    let name = t.name("the blend's name")?;
    t.want_word("AS", "AS after the blend's name")?;
    let mut def = Def { name, sources: Vec::new(), heads: Vec::new(), boost: Vec::new(), slots: Vec::new(), quotas: Vec::new(), spacing: Vec::new(), diversify: None, caps: Vec::new(), dedupe: None, k: default_k(), note: None };
    let mut utility: Option<Vec<(String, f64)>> = None;
    let mut seen: Vec<String> = Vec::new();
    while t.peek().is_some() {
        let clause = t.word();
        if clause.is_empty() {
            return Err(err(format!("expected a clause (SOURCES, HEADS, UTILITY, BOOST, SLOT, QUOTA, SPACING, DIVERSIFY, MAX, DEDUPE, WITH), not {}", t.next_text())));
        }
        if clause != "SLOT" && clause != "MAX" && seen.contains(&clause) {
            return Err(err(format!("{clause} is written twice")));
        }
        seen.push(clause.clone());
        t.i += 1;
        match clause.as_str() {
            "SOURCES" => t.list("the sources", |t| {
                let name = t.name("a source's name")?;
                t.want_word("AS", "AS after the source's name")?;
                let sql = t.group_text(&format!("source {name}'s query"))?;
                def.sources.push(Source { name, sql });
                Ok(())
            })?,
            "HEADS" => t.list("the heads", |t| {
                let name = t.name("a head (a column the sources name)")?;
                let mut h = Head { name, weight: 1.0, calibrate: Calibrate::Tmm, lo: None, missing: Missing::Neutral };
                loop {
                    if t.eat_word("CALIBRATE") {
                        h.calibrate = match t.word().as_str() {
                            "TMM" => Calibrate::Tmm,
                            "MINMAX" => Calibrate::Minmax,
                            "RANK" => Calibrate::Rank,
                            "RAW" => Calibrate::Raw,
                            _ => return Err(err(format!("CALIBRATE takes tmm, minmax, rank or raw, not {}", t.next_text()))),
                        };
                        t.i += 1;
                    } else if t.eat_word("LO") {
                        h.lo = Some(t.number("LO")?);
                    } else if t.eat_word("MISSING") {
                        h.missing = match t.word().as_str() {
                            "NEUTRAL" => Missing::Neutral,
                            "ZERO" => Missing::Zero,
                            _ => return Err(err(format!("MISSING takes neutral or zero, not {}", t.next_text()))),
                        };
                        t.i += 1;
                    } else {
                        break;
                    }
                }
                def.heads.push(h);
                Ok(())
            })?,
            "UTILITY" => {
                let mut terms = Vec::new();
                let mut first = true;
                loop {
                    let sign = if t.eat(&Token::Plus) {
                        1.0
                    } else if t.eat(&Token::Minus) {
                        -1.0
                    } else if first {
                        1.0
                    } else {
                        break;
                    };
                    first = false;
                    let w = if matches!(t.peek(), Some(Token::Number(..))) {
                        let w = t.number("a weight")?;
                        t.want(Token::Mul, "* between a weight and its head")?;
                        w
                    } else {
                        1.0
                    };
                    terms.push((t.name("a head")?, sign * w));
                }
                utility = Some(terms);
            }
            "BOOST" => t.list("the boosts", |t| {
                let source = t.name("a source")?;
                let by = t.number("the boost")?;
                def.boost.push(Boost { source, by });
                Ok(())
            })?,
            "SLOT" => {
                let slot = t.whole("the slot")?;
                t.want(Token::RArrow, "=> after the slot")?;
                def.slots.push(Slot { slot, when: t.pred()? });
            }
            "QUOTA" => t.list("the quotas", |t| {
                let when = t.pred()?;
                let mut q = Quota { when, min: 0.0, max: 100.0, top: None, if_any: false };
                if t.eat_word("BETWEEN") {
                    q.min = t.percent("the least share")?;
                    t.want_word("AND", "AND between the shares")?;
                    q.max = t.percent("the most share")?;
                } else if t.eat_word("AT") {
                    if t.eat_word("LEAST") {
                        q.min = t.percent("the least share")?;
                    } else {
                        t.want_word("MOST", "LEAST or MOST after AT")?;
                        q.max = t.percent("the most share")?;
                    }
                } else {
                    return Err(err(format!("expected BETWEEN, AT LEAST or AT MOST, not {}", t.next_text())));
                }
                if t.eat_word("IN") {
                    t.want_word("TOP", "TOP after IN")?;
                    q.top = Some(t.whole("the top rows")?);
                }
                if t.eat_word("IF") {
                    t.want_word("ANY", "ANY after IF")?;
                    q.if_any = true;
                }
                def.quotas.push(q);
                Ok(())
            })?,
            "SPACING" => t.list("the spacing", |t| {
                let when = t.pred()?;
                t.want_word("AT", "AT MOST n PER m ROWS")?;
                t.want_word("MOST", "AT MOST n PER m ROWS")?;
                let at_most = t.whole("n")?;
                t.want_word("PER", "PER m ROWS")?;
                let per = t.whole("m")?;
                t.eat_word("ROWS");
                def.spacing.push(Spacing { when, at_most, per });
                Ok(())
            })?,
            "DIVERSIFY" => {
                let lambda = t.number("λ")?;
                t.want_word("ON", "ON column after DIVERSIFY λ")?;
                def.diversify = Some(Diversify { lambda, on: t.column("the column")? });
            }
            "MAX" => {
                let at_most = t.whole("n")?;
                t.want_word("PER", "PER column after MAX n")?;
                def.caps.push(Cap { at_most, per: t.column("the column")? });
            }
            "DEDUPE" => {
                let threshold = t.number("the cosine")?;
                t.want_word("ON", "ON column after DEDUPE t")?;
                def.dedupe = Some(Dedupe { threshold, on: t.column("the column")? });
            }
            "WITH" => t.list("WITH", |t| {
                let opt = t.name("an option (k, note)")?;
                t.want(Token::RArrow, "=> after the option")?;
                match opt.as_str() {
                    "k" => def.k = t.whole("k")?,
                    "note" => match t.peek().cloned() {
                        Some(Token::SingleQuotedString(s)) => {
                            t.i += 1;
                            def.note = Some(s);
                        }
                        _ => return Err(err("note => 'text'")),
                    },
                    o => return Err(err(format!("WITH takes k and note, not {o}"))),
                }
                Ok(())
            })?,
            c => return Err(err(format!("{c} is not a clause of CREATE BLEND; the form is {CREATE_FORM}"))),
        }
    }
    if def.sources.is_empty() {
        return Err(err(format!("a blend needs SOURCES; the form is {CREATE_FORM}")));
    }
    if let Some(terms) = utility {
        for h in &mut def.heads {
            h.weight = 0.0;
        }
        for (name, w) in terms {
            match def.heads.iter_mut().find(|h| h.name == name) {
                Some(h) => h.weight += w,
                None => def.heads.push(Head { name, weight: w, calibrate: Calibrate::Tmm, lo: None, missing: Missing::Neutral }),
            }
        }
    }
    def.check()?;
    Ok(Some(Statement::Create { def, replace }))
}

/// A call of `FROM blend(...)`.
#[derive(Clone, Debug)]
pub struct Call {
    pub name: String,
    /// `name@3`: version 3; the current one without.
    pub version: Option<i64>,
    /// The sources' parameters: the query's, with the call's named
    /// arguments over them.
    pub params: Params,
    /// The rows to place (the blend's k without a LIMIT).
    pub limit: Option<usize>,
}

/// Whether `sql` may call `blend(...)`: it holds `blend` and a `(` after
/// it.
pub fn may_blend(sql: &str) -> bool {
    let b = sql.as_bytes();
    (0..b.len().saturating_sub(NAME.len())).any(|i| {
        b[i..i + NAME.len()].eq_ignore_ascii_case(NAME.as_bytes()) && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) && b[i + NAME.len()..].iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'(')
    })
}

/// The blends of `set`: each current version's definition, and the
/// function that holds it.
pub fn defs(set: &FunctionSet) -> Vec<(Def, Arc<Function>)> {
    set.currents().filter(|f| !f.dropped).filter_map(|f| Def::of_body(&f.def.body).map(|d| (d, f.clone()))).collect()
}

/// Every version of blend `name`, oldest first, tombstones as `None`.
pub fn versions(set: &FunctionSet, name: &str) -> Vec<(Option<Def>, Arc<Function>)> {
    set.all().filter(|f| f.def.name.eq_ignore_ascii_case(name)).map(|f| ((!f.dropped).then(|| Def::of_body(&f.def.body)).flatten(), f.clone())).filter(|(d, f)| d.is_some() || f.dropped).collect()
}

/// The definition of blend `name`: version `version`, or the current one.
pub fn resolve(set: &FunctionSet, name: &str, version: Option<i64>) -> Result<(Def, i64)> {
    let f = match version {
        Some(v) => set.get(name, v).ok_or_else(|| err(format!("blend {name} has no version {v}")))?,
        None => set.current(name).ok_or_else(|| err(format!("no blend named {name} (CREATE BLEND makes one)")))?,
    };
    if f.dropped {
        return Err(err(format!("blend {name}@{} was dropped", f.version)));
    }
    let d = Def::of_body(&f.def.body).ok_or_else(|| err(format!("function {name} is not a blend")))?;
    Ok((d, f.version))
}

/// Whether `def` may be written over `set`: a function of its name must
/// be a blend, and is replaced only by `OR REPLACE`.
pub fn may_define(set: &FunctionSet, def: &Def, replace: bool) -> Result<()> {
    def.check()?;
    if let Some(f) = set.current(&def.name).filter(|f| !f.dropped) {
        if Def::of_body(&f.def.body).is_none() {
            return Err(err(format!("function {} exists and is not a blend: name the blend otherwise", def.name)));
        }
        if !replace {
            return Err(err(format!("blend {} exists: CREATE OR REPLACE BLEND {} makes its next version", def.name, def.name)));
        }
    }
    Ok(())
}

/// The row `CREATE BLEND` answers: the blend and the version it wrote.
pub fn made(def: &Def, version: i64) -> ResultSet {
    ResultSet {
        columns: vec!["blend".into(), "version".into(), "sources".into(), "k".into()],
        rows: vec![vec![Value::Str(def.name.clone()), Value::Int(version), Value::Int(def.sources.len() as i64), Value::Int(def.k as i64)]],
        keys: Vec::new(),
        scores: Vec::new(),
        parts: None,
        stats: Default::default(),
    }
}

/// The blend a query reads, `FROM blend('name')`, without its arguments.
pub fn call_name(q: &sql::Query) -> Option<String> {
    let sql::SetExpr::Select(s) = q.body.as_ref() else { return None };
    s.from.iter().find_map(|f| {
        let sql::TableFactor::Table { name, args: Some(args), .. } = &f.relation else { return None };
        if !name.to_string().eq_ignore_ascii_case(NAME) {
            return None;
        }
        match args.args.first() {
            Some(FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Value(v)))) => match &v.value {
                sql::Value::SingleQuotedString(s) => Some(split_version(s).0),
                _ => None,
            },
            _ => Some(String::new()),
        }
    })
}

/// The blends the query `text` reads (none when it does not parse).
pub fn names_sql(text: &str) -> Vec<String> {
    if !may_blend(text) {
        return Vec::new();
    }
    match crate::query::sql::parse_query(text) {
        Ok(q) => call_name(&q).into_iter().collect(),
        Err(_) => Vec::new(),
    }
}

fn split_version(s: &str) -> (String, Option<i64>) {
    match s.rsplit_once('@') {
        Some((n, v)) if v.parse::<i64>().is_ok() => (n.trim().to_ascii_lowercase(), v.parse().ok()),
        _ => (s.trim().to_ascii_lowercase(), None),
    }
}

/// `SELECT columns FROM blend(...) [LIMIT k]`, its call and the columns
/// it asks for; `None` for a query that reads no blend.
pub fn table_call(q: &sql::Query, params: &Params) -> Result<Option<(Call, Vec<(String, String)>)>> {
    let sql::SetExpr::Select(s) = q.body.as_ref() else { return Ok(None) };
    let [from] = s.from.as_slice() else { return Ok(None) };
    let sql::TableFactor::Table { name, args: Some(args), .. } = &from.relation else { return Ok(None) };
    if !name.to_string().eq_ignore_ascii_case(NAME) {
        return Ok(None);
    }
    if !from.joins.is_empty() || s.selection.is_some() || q.order_by.is_some() || !matches!(&s.group_by, sql::GroupByExpr::Expressions(g, _) if g.is_empty()) || q.with.is_some() {
        return Err(err(format!("FROM {NAME}(...) is read as written, its rows in their slots: it takes no join, WHERE, ORDER BY, GROUP BY or WITH; filter and rank inside its sources")));
    }
    let mut pos = Vec::new();
    let mut named: Vec<(String, Expr)> = Vec::new();
    for a in &args.args {
        match a {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => pos.push(e.clone()),
            FunctionArg::Named { name, arg: FunctionArgExpr::Expr(e), .. } => named.push((name.value.to_ascii_lowercase(), e.clone())),
            FunctionArg::ExprNamed { name: Expr::Identifier(name), arg: FunctionArgExpr::Expr(e), .. } => named.push((name.value.to_ascii_lowercase(), e.clone())),
            _ => return Err(err(format!("{USAGE}: arguments are values or parameters, named"))),
        }
    }
    let [first] = pos.as_slice() else {
        return Err(err(format!("{USAGE}: the blend's name first, then named arguments")));
    };
    let Value::Str(id) = crate::query::compound::value_of(first, params)? else {
        return Err(err(format!("{USAGE}: the blend's name is text")));
    };
    let (bname, version) = split_version(&id);
    let mut p = params.clone();
    for (n, e) in &named {
        p.0.insert(n.clone(), crate::query::compound::value_of(e, params)?);
    }
    let limit = match &q.limit_clause {
        None => None,
        Some(sql::LimitClause::LimitOffset { limit: None, offset: None, .. }) => None,
        Some(sql::LimitClause::LimitOffset { limit: Some(l), offset: None, .. }) => {
            let n = crate::query::compound::value_of(l, params)?.as_f64().filter(|x| *x >= 1.0 && x.fract() == 0.0 && *x <= MAX_K as f64).ok_or_else(|| err(format!("LIMIT is a whole number, 1 to {MAX_K}")))?;
            Some(n as usize)
        }
        _ => return Err(err(format!("FROM {NAME}(...) takes a LIMIT, not an OFFSET: the slots are its order"))),
    };
    let mut cols = Vec::new();
    for item in &s.projection {
        match item {
            sql::SelectItem::Wildcard(_) => cols.push(("*".to_string(), "*".to_string())),
            sql::SelectItem::UnnamedExpr(Expr::Identifier(i)) => cols.push((i.value.clone(), i.value.clone())),
            sql::SelectItem::ExprWithAlias { expr: Expr::Identifier(i), alias } => cols.push((i.value.clone(), alias.value.clone())),
            i => return Err(err(format!("FROM {NAME}(...) has its sources' columns and {}, not {i}", COLUMNS.join(", ")))),
        }
    }
    Ok(Some((Call { name: bname, version, params: p, limit }, cols)))
}

/// [`table_call`] of the text `sql`; `None` for one that reads no blend
/// or is not a query.
pub fn table_call_sql(sql: &str, params: &Params) -> Result<Option<(Call, Vec<(String, String)>)>> {
    if !may_blend(sql) {
        return Ok(None);
    }
    match crate::query::sql::parse_query(sql) {
        Ok(q) => table_call(&q, params),
        Err(_) => Ok(None),
    }
}

/// What a blend asks of its run.
#[derive(Clone, Copy, Debug, Default)]
pub struct Want {
    /// Each rule's cost: a fill without it, for each.
    pub costs: bool,
}

/// One source's rows, as its query answered them.
pub struct Input {
    pub rs: ResultSet,
    pub took_ms: f64,
}

/// A row a source gave.
#[derive(Clone, Debug)]
pub struct Cand {
    pub source: usize,
    /// From 1, in its source.
    pub rank: usize,
    pub key: Value,
    pub row: Vec<Value>,
    /// Each head's value as the source gave it.
    pub raw: Vec<Option<f64>>,
    /// Each head's calibrated value, times its weight.
    pub parts: Vec<f64>,
    pub boost: f64,
    pub utility: f64,
    /// Other sources that gave a row of the same key.
    pub also: Vec<usize>,
}

/// A row placed.
#[derive(Clone, Debug)]
pub struct Placed {
    pub cand: usize,
    /// From 1.
    pub slot: usize,
    /// Its utility less what diversity took off.
    pub adjusted: f64,
    /// `pin`, `quota`, `utility` or `relaxed`.
    pub kind: &'static str,
    pub why: String,
}

/// A rule's cost: what the slate gives up for it.
#[derive(Clone, Debug)]
pub struct Cost {
    pub rule: String,
    /// The share of the discounted utility without the rule it costs.
    pub share: f64,
    /// Slots that hold another row without it.
    pub moved: usize,
}

/// A quota, as the slate met it.
#[derive(Clone, Debug)]
pub struct QuotaMet {
    pub rule: String,
    pub top: usize,
    pub got: usize,
    pub least: usize,
    pub most: usize,
    /// No row of its kind (`IF ANY`).
    pub idle: bool,
    pub met: bool,
}

/// A blend's run: its rows in their slots, and why.
#[derive(Clone, Debug)]
pub struct Blended {
    pub name: String,
    pub version: i64,
    pub k: usize,
    pub sources: Vec<String>,
    /// Rows and time per source.
    pub source_rows: Vec<usize>,
    pub source_ms: Vec<f64>,
    pub heads: Vec<Head>,
    /// Every source's columns, each once, in the order they first come.
    pub columns: Vec<String>,
    col_of: Vec<HashMap<String, usize>>,
    pub cands: Vec<Cand>,
    pub placed: Vec<Placed>,
    /// Rows worth more than one placed that were not, and why.
    pub passed: Vec<(usize, String)>,
    pub quotas: Vec<QuotaMet>,
    pub costs: Vec<Cost>,
    pub fill_ms: f64,
}

/// What the fill reads of each candidate, made once.
struct Ctx<'a> {
    def: &'a Def,
    k: usize,
    cands: &'a [Cand],
    order: Vec<usize>,
    slot_of: HashMap<usize, usize>,
    slot_hit: Vec<Vec<bool>>,
    quota_hit: Vec<Vec<bool>>,
    space_hit: Vec<Vec<bool>>,
    cap_val: Vec<Vec<Option<String>>>,
    /// Each cap's value of each row as a number (`None` for no value),
    /// so that counting is by index rather than by text.
    cap_id: Vec<Vec<Option<usize>>>,
    div: Vec<Like>,
    dedupe: Vec<Option<Vec<f32>>>,
    scale: f64,
}

/// What a row is like, for diversity.
#[derive(Clone)]
enum Like {
    None,
    Vector(Vec<f32>),
    Text(String),
}

fn unit(v: &[f32]) -> Option<Vec<f32>> {
    let n = v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    (n > 0.0 && n.is_finite()).then(|| v.iter().map(|x| (*x as f64 / n) as f32).collect())
}

/// The dot product of two unit vectors, over the shorter's length: eight
/// lanes at a time, which the compiler keeps in vector registers.
fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let ((xa, ta), (xb, tb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    let mut lanes = [0f32; 8];
    for (x, y) in xa.iter().zip(xb) {
        for i in 0..8 {
            lanes[i] += x[i] * y[i];
        }
    }
    lanes.iter().map(|&v| v as f64).sum::<f64>() + ta.iter().zip(tb).map(|(x, y)| (*x as f64) * (*y as f64)).sum::<f64>()
}

fn like(a: &Like, b: &Like) -> f64 {
    match (a, b) {
        (Like::Vector(x), Like::Vector(y)) if x.len() == y.len() => cosine(x, y).max(0.0),
        (Like::Text(x), Like::Text(y)) => (x == y) as u8 as f64,
        _ => 0.0,
    }
}

/// A value as text, as rules compare it.
pub fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) | Value::Timestamp(n) => n.to_string(),
        Value::Float(x) => num(*x),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        v => format!("{v:?}"),
    }
}

fn json_text(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.as_f64().map_or_else(|| n.to_string(), num),
        Json::Bool(b) => b.to_string(),
        Json::Null => String::new(),
        v => v.to_string(),
    }
}

fn json_sql(v: &Json) -> String {
    match v {
        Json::String(s) => quote(s),
        v => json_text(v),
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A number as it reads back: an integer without a point.
fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// A value as JSON, for the receipts (a vector as its length).
pub fn value_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Int(n) => json!(n),
        Value::Float(x) if x.is_finite() => json!(x),
        Value::Float(_) => Json::Null,
        Value::Str(s) => json!(s),
        Value::Bool(b) => json!(b),
        Value::Timestamp(t) => json!(crate::timestamp::format(*t)),
        Value::Vector(v) => json!(format!("vector of {}", v.len())),
        Value::List(l) => Json::Array(l.iter().map(value_json).collect()),
    }
}

fn round(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// Blends the rows of `inputs` (one per source, in the definition's
/// order) into `k` slots.
pub fn blend(def: &Def, version: i64, k: usize, inputs: Vec<Input>, want: Want) -> Result<Blended> {
    let t0 = std::time::Instant::now();
    if inputs.len() != def.sources.len() {
        return Err(err("a blend's run needs every source's rows"));
    }
    // A LIMIT other than the blend's k may leave a quota short: the fill
    // keeps what it can and the receipts say which is not met.
    let k = k.clamp(1, MAX_K);
    let heads = def.heads();
    let mut columns: Vec<String> = Vec::new();
    let mut col_of = Vec::new();
    for inp in &inputs {
        let mut m = HashMap::new();
        for (i, c) in inp.rs.columns.iter().enumerate() {
            m.entry(c.to_ascii_lowercase()).or_insert(i);
            if !columns.iter().any(|x| x.eq_ignore_ascii_case(c)) && !COLUMNS.contains(&c.to_ascii_lowercase().as_str()) {
                columns.push(c.clone());
            }
        }
        col_of.push(m);
    }
    // Every row, its heads raw.
    let mut cands: Vec<Cand> = Vec::new();
    for (s, inp) in inputs.iter().enumerate() {
        let rs = &inp.rs;
        let scores = rs.scores.len() == rs.rows.len();
        for (i, row) in rs.rows.iter().enumerate() {
            let key = match rs.keys.get(i) {
                Some(v) if *v != Value::Null => v.clone(),
                _ => row.first().cloned().unwrap_or(Value::Null),
            };
            let raw = heads
                .iter()
                .map(|h| match col_of[s].get(&h.name) {
                    Some(&c) => match &row[c] {
                        Value::Timestamp(t) => Some(*t as f64),
                        v => v.as_f64().filter(|x| x.is_finite()),
                    },
                    None if h.name == "score" && scores => Some(rs.scores[i]).filter(|x| x.is_finite()),
                    None => None,
                })
                .collect();
            let boost = def.boost.iter().filter(|b| b.source.eq_ignore_ascii_case(&def.sources[s].name)).map(|b| b.by).sum();
            cands.push(Cand { source: s, rank: i + 1, key, row: row.clone(), raw, parts: vec![0.0; heads.len()], boost, utility: 0.0, also: Vec::new() });
        }
    }
    // Calibrated, per source, then the missing filled.
    let mut cal: Vec<Vec<Option<f64>>> = vec![vec![None; heads.len()]; cands.len()];
    for (h, head) in heads.iter().enumerate() {
        for s in 0..def.sources.len() {
            let idx: Vec<usize> = (0..cands.len()).filter(|&c| cands[c].source == s && cands[c].raw[h].is_some()).collect();
            if idx.is_empty() {
                continue;
            }
            let vals: Vec<f64> = idx.iter().map(|&c| cands[c].raw[h].unwrap_or(0.0)).collect();
            let (lo_c, hi_c) = vals.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
            let mut by_rank: Vec<usize> = (0..vals.len()).collect();
            by_rank.sort_by(|&a, &b| vals[b].total_cmp(&vals[a]).then(a.cmp(&b)));
            let mut rank_of = vec![0usize; vals.len()];
            for (r, &j) in by_rank.iter().enumerate() {
                rank_of[j] = r + 1;
            }
            for (j, &c) in idx.iter().enumerate() {
                let x = vals[j];
                cal[c][h] = Some(match head.calibrate {
                    Calibrate::Tmm => {
                        let lo = head.lo.unwrap_or(0.0);
                        if hi_c > lo { (x - lo) / (hi_c - lo) } else { 0.0 }
                    }
                    Calibrate::Minmax => {
                        if hi_c > lo_c { (x - lo_c) / (hi_c - lo_c) } else { 1.0 }
                    }
                    Calibrate::Rank => (RRF_K + 1.0) / (RRF_K + rank_of[j] as f64),
                    Calibrate::Raw => x,
                });
            }
        }
        let present: Vec<f64> = cal.iter().filter_map(|c| c[h]).collect();
        let neutral = if present.is_empty() { 0.0 } else { present.iter().sum::<f64>() / present.len() as f64 };
        for (c, cand) in cands.iter_mut().enumerate() {
            let v = cal[c][h].unwrap_or(match head.missing {
                Missing::Neutral => neutral,
                Missing::Zero => 0.0,
            });
            cand.parts[h] = head.weight * v;
        }
    }
    for c in &mut cands {
        c.utility = c.parts.iter().sum::<f64>() + c.boost;
        if !c.utility.is_finite() {
            c.utility = f64::NEG_INFINITY;
        }
    }
    // One row per key: the one worth more.
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut keep = vec![true; cands.len()];
    for c in 0..cands.len() {
        if cands[c].key == Value::Null {
            continue;
        }
        let kt = text(&cands[c].key);
        match by_key.get(&kt).copied() {
            None => {
                by_key.insert(kt, c);
            }
            Some(o) => {
                let (win, lose) = if cands[c].utility > cands[o].utility { (c, o) } else { (o, c) };
                keep[lose] = false;
                let s = cands[lose].source;
                if !cands[win].also.contains(&s) && cands[win].source != s {
                    cands[win].also.push(s);
                }
                let mut moved = std::mem::take(&mut cands[lose].also);
                cands[win].also.append(&mut moved);
                by_key.insert(text(&cands[win].key), win);
            }
        }
    }
    let mut kept = Vec::new();
    for (c, cand) in cands.into_iter().enumerate() {
        if keep[c] {
            kept.push(cand);
        }
    }
    let cands = kept;
    let get = |c: &Cand, col: &str| -> Value {
        if col.eq_ignore_ascii_case("source") {
            return Value::Str(def.sources[c.source].name.clone());
        }
        col_of[c.source].get(&col.to_ascii_lowercase()).map_or(Value::Null, |&i| c.row[i].clone())
    };
    let hit = |p: &Pred| -> Vec<bool> {
        let want = json_text(&p.value);
        cands.iter().map(|c| text(&get(c, &p.column)) == want).collect()
    };
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| cands[b].utility.total_cmp(&cands[a].utility).then(cands[a].source.cmp(&cands[b].source)).then(cands[a].rank.cmp(&cands[b].rank)));
    let finite: Vec<f64> = cands.iter().map(|c| c.utility).filter(|u| u.is_finite()).collect();
    let spread = finite.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - finite.iter().cloned().fold(f64::INFINITY, f64::min);
    let cap_val: Vec<Vec<Option<String>>> = def.caps.iter().map(|cp| cands.iter().map(|c| Some(text(&get(c, &cp.per))).filter(|t| !t.is_empty())).collect()).collect();
    let ctx = Ctx {
        def,
        k,
        cands: &cands,
        order,
        slot_of: def.slots.iter().enumerate().map(|(i, s)| (s.slot, i)).collect(),
        slot_hit: def.slots.iter().map(|s| hit(&s.when)).collect(),
        quota_hit: def.quotas.iter().map(|q| hit(&q.when)).collect(),
        space_hit: def.spacing.iter().map(|s| hit(&s.when)).collect(),
        cap_id: cap_val
            .iter()
            .map(|vals| {
                let mut ids: HashMap<&str, usize> = HashMap::new();
                vals.iter().map(|v| v.as_deref().map(|t| { let n = ids.len(); *ids.entry(t).or_insert(n) })).collect()
            })
            .collect(),
        cap_val,
        div: match &def.diversify {
            Some(d) => cands
                .iter()
                .map(|c| match get(c, &d.on) {
                    Value::Vector(v) => unit(&v).map_or(Like::None, Like::Vector),
                    Value::Null => Like::None,
                    v => Like::Text(text(&v)),
                })
                .collect(),
            None => vec![Like::None; cands.len()],
        },
        dedupe: match &def.dedupe {
            Some(d) => cands.iter().map(|c| get(c, &d.on).as_vector().and_then(unit)).collect(),
            None => vec![None; cands.len()],
        },
        scale: if spread.is_finite() && spread > 1e-12 { spread } else { 1.0 },
    };
    let base = fill(&ctx, None);
    let mut costs = Vec::new();
    if want.costs {
        let worth = |f: &Fill| -> f64 { f.placed.iter().enumerate().map(|(i, p)| cands[p.cand].utility.max(0.0) / ((i + 2) as f64).log2()).sum() };
        let w0 = worth(&base);
        for r in def.rules() {
            let without = fill(&ctx, Some(r));
            let w = worth(&without);
            let moved = (0..k).filter(|&i| base.placed.get(i).map(|p| p.cand) != without.placed.get(i).map(|p| p.cand)).count();
            costs.push(Cost { rule: def.rule_sql(r), share: if w > 0.0 { round(((w - w0) / w).max(0.0)) } else { 0.0 }, moved });
        }
    }
    // Rows passed over: worth more than the least placed.
    let least = base.placed.iter().map(|p| cands[p.cand].utility).fold(f64::INFINITY, f64::min);
    let placed_set: std::collections::HashSet<usize> = base.placed.iter().map(|p| p.cand).collect();
    let passed: Vec<(usize, String)> = ctx
        .order
        .iter()
        .filter(|&&c| !placed_set.contains(&c) && cands[c].utility > least)
        .take(PASSED)
        .map(|&c| (c, base.block[c].map_or_else(|| "worth less than the rows placed once diversity counted".into(), |b| b.text(&ctx, c))))
        .collect();
    let quotas = def
        .quotas
        .iter()
        .enumerate()
        .map(|(q, quota)| {
            let n = quota.over(k);
            let got = base.placed.iter().filter(|p| p.slot <= n && ctx.quota_hit[q][p.cand]).count();
            let (least, most) = quota.bounds(k);
            let idle = quota.if_any && !ctx.quota_hit[q].iter().any(|h| *h);
            QuotaMet { rule: quota.sql(), top: n, got, least, most, idle, met: idle || (got >= least.min(base.placed.len().min(n)) && got <= most) }
        })
        .collect();
    let n_src = def.sources.len();
    Ok(Blended {
        name: def.name.clone(),
        version,
        k,
        sources: def.sources.iter().map(|s| s.name.clone()).collect(),
        source_rows: inputs.iter().map(|i| i.rs.rows.len()).collect(),
        source_ms: inputs.iter().map(|i| i.took_ms).collect(),
        heads,
        columns,
        col_of,
        placed: base.placed,
        passed,
        quotas,
        costs,
        cands,
        fill_ms: t0.elapsed().as_secs_f64() * 1e3,
    })
    .map(|mut b| {
        b.sources.truncate(n_src);
        b
    })
}

/// A fill's rows, and why each row it passed over was.
struct Fill {
    placed: Vec<Placed>,
    block: Vec<Option<Block>>,
}

/// Why a row worth more than the one placed in a slot was not placed
/// there: kept as numbers while filling, written out for the rows the
/// receipts show ([`Block::text`]).
#[derive(Clone, Copy)]
enum Block {
    /// The slot was kept by the def's `slots[pin]`.
    Pin { slot: usize, pin: usize },
    /// The slot went to a quota's least share.
    Forced { slot: usize },
    /// Quota `q` was at its most.
    Full(usize),
    /// Cap `ci` held as many of its value as it allows.
    Cap(usize),
    Spacing,
    /// Worth less once diversity counted.
    Diversity,
    /// A near copy of the row in `slot`.
    Copy { slot: usize, cos: f64 },
}

impl Block {
    fn text(self, ctx: &Ctx, c: usize) -> String {
        let def = ctx.def;
        match self {
            Block::Pin { slot, pin } => format!("slot {slot} is kept for {}", def.slots[pin].when.sql()),
            Block::Forced { slot } => format!("slot {slot} went to a quota's least share"),
            Block::Full(q) => format!("QUOTA {} is full", def.quotas[q].sql()),
            Block::Cap(ci) => format!("MAX {} PER {}: {} has them", def.caps[ci].at_most, def.caps[ci].per, ctx.cap_val[ci][c].clone().unwrap_or_default()),
            Block::Spacing => "SPACING: too near another of its kind".to_string(),
            Block::Diversity => "worth less than the row placed once diversity counted".to_string(),
            Block::Copy { slot, cos } => format!("a near copy of slot {slot} (cosine {cos:.3})"),
        }
    }
}

/// Why a row can not go in a slot now, if it can not: bits of a mask, in
/// the order the rules are set aside when no row fits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bar {
    Spacing = 1,
    Cap = 2,
    Forced = 4,
    Pin = 8,
    QuotaMax = 16,
}

fn fill(ctx: &Ctx, skip: Option<Rule>) -> Fill {
    let (def, k, cands) = (ctx.def, ctx.k, ctx.cands);
    let n = cands.len();
    let mut used = vec![false; n];
    let mut gone: Vec<Option<(usize, f64)>> = vec![None; n];
    let mut block: Vec<Option<Block>> = vec![None; n];
    let mut placed: Vec<Placed> = Vec::new();
    let mut quota_count = vec![0usize; def.quotas.len()];
    let mut cap_count: Vec<Vec<usize>> = ctx.cap_id.iter().map(|ids| vec![0; ids.iter().flatten().max().map_or(0, |m| m + 1)]).collect();
    let quota_on: Vec<bool> = def.quotas.iter().enumerate().map(|(q, quota)| skip != Some(Rule::Quota(q)) && !(quota.if_any && !ctx.quota_hit[q].iter().any(|h| *h))).collect();
    let pin_at = |slot: usize| -> Option<usize> { ctx.slot_of.get(&slot).copied().filter(|&i| skip != Some(Rule::Slot(i))) };
    let lambda = match (&def.diversify, skip) {
        (_, Some(Rule::Diversify)) => 0.0,
        (Some(d), _) => d.lambda,
        _ => 0.0,
    };
    let dedupe = def.dedupe.as_ref().filter(|_| skip != Some(Rule::Dedupe));
    // Each row's likeness to the rows placed (its most, and that row's
    // slot), and whether it is a near copy of one: brought up to date only
    // for the rows a slot looks at, `seen[c]` the rows placed it was
    // compared with, rather than every row's after each placement.
    let mut like_most: Vec<(f64, usize)> = vec![(0.0, 0); n];
    let mut seen = vec![0usize; n];
    let catch_up = |c: usize, placed: &[Placed], seen: &mut [usize], like_most: &mut [(f64, usize)], gone: &mut [Option<(usize, f64)>], block: &mut [Option<Block>]| {
        for p in &placed[seen[c]..] {
            if lambda > 0.0 {
                let l = like(&ctx.div[c], &ctx.div[p.cand]);
                if l > like_most[c].0 {
                    like_most[c] = (l, p.slot);
                }
            }
            if let (Some(d), Some(v), Some(w)) = (dedupe, &ctx.dedupe[p.cand], &ctx.dedupe[c]) {
                let cos = cosine(v, w);
                if w.len() == v.len() && cos >= d.threshold {
                    gone[c] = Some((p.slot, cos));
                    block[c] = Some(Block::Copy { slot: p.slot, cos });
                    break;
                }
            }
        }
        seen[c] = placed.len();
    };
    // A slot is tried again when every row it would take turned out a near
    // copy of a row placed: they are gone, and the rules are counted anew.
    let mut slot = 1;
    while slot <= k {
        let pin = pin_at(slot);
        // Quotas that need their kind now, and those full.
        let mut forced: Vec<usize> = Vec::new();
        let mut full: Vec<usize> = Vec::new();
        for (q, quota) in def.quotas.iter().enumerate() {
            if !quota_on[q] || slot > quota.over(k) {
                continue;
            }
            let (least, most) = quota.bounds(k);
            if quota_count[q] >= most {
                full.push(q);
            }
            if quota_count[q] < least {
                let free = (slot..=quota.over(k)).filter(|&j| match pin_at(j) {
                    None => true,
                    Some(i) => def.slots[i].when.same(&quota.when),
                }).count();
                // Only what some row left can meet.
                let left = ctx.order.iter().any(|&c| !used[c] && gone[c].is_none() && ctx.quota_hit[q][c]);
                if free > 0 && least - quota_count[q] >= free && left {
                    forced.push(q);
                }
            }
        }
        // Spacing rules whose kind has as many as they allow in the rows
        // just above this slot.
        let spaced: Vec<usize> = def
            .spacing
            .iter()
            .enumerate()
            .filter(|&(si, sp)| {
                let from = slot.saturating_sub(sp.per - 1).max(1);
                skip != Some(Rule::Spacing(si)) && placed.iter().filter(|p| p.slot >= from && ctx.space_hit[si][p.cand]).count() >= sp.at_most
            })
            .map(|(si, _)| si)
            .collect();
        let capped = |ci: usize, c: usize| -> bool {
            skip != Some(Rule::Cap(ci)) && ctx.cap_id[ci][c].is_some_and(|v| cap_count[ci][v] >= def.caps[ci].at_most)
        };
        let bars = |c: usize| -> u8 {
            let mut b = 0u8;
            if pin.is_some_and(|i| !ctx.slot_hit[i][c]) {
                b |= Bar::Pin as u8;
            }
            if forced.iter().any(|&q| !ctx.quota_hit[q][c]) {
                b |= Bar::Forced as u8;
            }
            if full.iter().any(|&q| ctx.quota_hit[q][c]) {
                b |= Bar::QuotaMax as u8;
            }
            if (0..def.caps.len()).any(|ci| capped(ci, c)) {
                b |= Bar::Cap as u8;
            }
            if spaced.iter().any(|&si| ctx.space_hit[si][c]) {
                b |= Bar::Spacing as u8;
            }
            b
        };
        let open: Vec<usize> = ctx.order.iter().copied().filter(|&c| !used[c] && gone[c].is_none() && cands[c].utility.is_finite()).collect();
        if open.is_empty() {
            break;
        }
        let barred: Vec<u8> = open.iter().map(|&c| bars(c)).collect();
        // The rules set aside, least first, until some row fits.
        let mut aside: Vec<Bar> = Vec::new();
        let mut aside_mask = 0u8;
        let ladder = [Bar::Spacing, Bar::Cap, Bar::Forced, Bar::Pin, Bar::QuotaMax];
        let mut fits: Vec<usize> = (0..open.len()).filter(|&i| barred[i] == 0).collect();
        for b in ladder {
            if !fits.is_empty() {
                break;
            }
            aside.push(b);
            aside_mask |= b as u8;
            fits = (0..open.len()).filter(|&i| barred[i] & !aside_mask == 0).collect();
        }
        if fits.is_empty() {
            break;
        }
        // The best of them, less its likeness to the rows placed.
        let mut best: Option<(usize, f64, f64, usize)> = None;
        for &i in &fits {
            let c = open[i];
            // In order of utility, and diversity only takes off: a row
            // worth no more than the best adjusted so far cannot beat it.
            if best.is_some_and(|b| cands[c].utility <= b.1) {
                break;
            }
            catch_up(c, &placed, &mut seen, &mut like_most, &mut gone, &mut block);
            if gone[c].is_some() {
                continue;
            }
            let (most, like_slot) = if lambda > 0.0 { like_most[c] } else { (0.0, 0) };
            let adj = cands[c].utility - lambda * ctx.scale * most;
            if best.is_none_or(|b| adj > b.1) {
                best = Some((c, adj, lambda * ctx.scale * most, like_slot));
            }
        }
        // Every row that fit was a near copy: the slot again without them.
        let Some((c, adj, off, like_slot)) = best else { continue };
        let (kind, why) = if !aside.is_empty() {
            let names: Vec<&str> = aside
                .iter()
                .map(|b| match b {
                    Bar::Spacing => "SPACING",
                    Bar::Cap => "MAX PER",
                    Bar::Forced => "a quota's least",
                    Bar::Pin => "the slot's SLOT",
                    Bar::QuotaMax => "a quota's most",
                })
                .collect();
            ("relaxed", format!("no row fit every rule: {} set aside", names.join(", ")))
        } else if let Some(i) = pin {
            ("pin", format!("slot {slot} is kept for {}", def.slots[i].when.sql()))
        } else if let Some(&q) = forced.first() {
            let (least, _) = def.quotas[q].bounds(k);
            ("quota", format!("{} needs {least} of the top {}: {} so far", def.quotas[q].when.sql(), def.quotas[q].over(k), quota_count[q]))
        } else if off > 1e-12 {
            ("utility", format!("the best utility left once diversity counted ({:.3} off for being like slot {like_slot})", off))
        } else {
            ("utility", "the best utility left".to_string())
        };
        // Why rows worth more were not placed here.
        for (i, &o) in open.iter().enumerate() {
            if o == c || cands[o].utility <= cands[c].utility || gone[o].is_some() {
                continue;
            }
            // The first rule not set aside that barred it, in the order
            // the bars are named: the slot, a quota's least, its most, a
            // cap, spacing.
            let on = barred[i] & !aside_mask;
            let reason = if on & Bar::Pin as u8 != 0 {
                Block::Pin { slot, pin: pin.unwrap_or(0) }
            } else if on & Bar::Forced as u8 != 0 {
                Block::Forced { slot }
            } else if on & Bar::QuotaMax as u8 != 0 {
                Block::Full(full.iter().find(|&&q| ctx.quota_hit[q][o]).copied().unwrap_or(0))
            } else if on & Bar::Cap as u8 != 0 {
                Block::Cap((0..def.caps.len()).find(|&ci| capped(ci, o)).unwrap_or(0))
            } else if on & Bar::Spacing as u8 != 0 {
                Block::Spacing
            } else {
                Block::Diversity
            };
            block[o] = Some(reason);
        }
        used[c] = true;
        for (q, h) in ctx.quota_hit.iter().enumerate() {
            if h[c] {
                quota_count[q] += 1;
            }
        }
        for (ci, ids) in ctx.cap_id.iter().enumerate() {
            if let Some(v) = ids[c] {
                cap_count[ci][v] += 1;
            }
        }
        placed.push(Placed { cand: c, slot, adjusted: adj, kind, why });
        slot += 1;
    }
    // The rows the receipts show as passed over (worth more than the least
    // placed, best first) are compared with every row placed, so that a
    // near copy says so.
    let least = placed.iter().map(|p| cands[p.cand].utility).fold(f64::INFINITY, f64::min);
    for &c in ctx.order.iter().filter(|&&c| !used[c] && cands[c].utility > least).take(PASSED) {
        if gone[c].is_none() {
            catch_up(c, &placed, &mut seen, &mut like_most, &mut gone, &mut block);
        }
    }
    Fill { placed, block }
}

impl Blended {
    /// The value of output column `col` of the row in slot `p`.
    fn cell(&self, p: &Placed, col: &str) -> Value {
        let c = &self.cands[p.cand];
        match col.to_ascii_lowercase().as_str() {
            "slot" => Value::Int(p.slot as i64),
            "source" => Value::Str(self.sources[c.source].clone()),
            "key" => c.key.clone(),
            "utility" => Value::Float(round(c.utility)),
            "blend_decision" => Value::Str(p.why.clone()),
            l => self.col_of[c.source].get(l).map_or(Value::Null, |&i| c.row[i].clone()),
        }
    }

    /// The columns `*` stands for.
    pub fn star(&self) -> Vec<String> {
        let mut cols = vec!["slot".to_string(), "source".to_string()];
        cols.extend(self.columns.iter().cloned());
        cols.push("utility".into());
        cols.push("blend_decision".into());
        cols
    }

    /// Its rows as a table of the columns asked for: (column, its name).
    pub fn result(&self, asked: &[(String, String)]) -> Result<ResultSet> {
        let mut cols: Vec<(String, String)> = Vec::new();
        for (c, alias) in asked {
            if c == "*" {
                cols.extend(self.star().into_iter().map(|s| (s.clone(), s)));
                continue;
            }
            let known = COLUMNS.contains(&c.to_ascii_lowercase().as_str()) || self.columns.iter().any(|x| x.eq_ignore_ascii_case(c));
            if !known {
                return Err(err(format!("blend {} has no column {c}: it has {}", self.name, self.star().join(", "))));
            }
            cols.push((c.clone(), alias.clone()));
        }
        let rows = self.placed.iter().map(|p| cols.iter().map(|(c, _)| self.cell(p, c)).collect()).collect();
        let mut rs = ResultSet { columns: cols.iter().map(|c| c.1.clone()).collect(), rows, keys: self.placed.iter().map(|p| self.cands[p.cand].key.clone()).collect(), scores: self.placed.iter().map(|p| round(self.cands[p.cand].utility)).collect(), parts: None, stats: Default::default() };
        rs.stats.plan = self.lines();
        Ok(rs)
    }

    /// Each source's share of the rows placed.
    pub fn shares(&self) -> Vec<usize> {
        let mut n = vec![0; self.sources.len()];
        for p in &self.placed {
            n[self.cands[p.cand].source] += 1;
        }
        n
    }

    /// The plan lines: what each source gave, and what the fill did.
    pub fn lines(&self) -> Vec<String> {
        let mut l = Vec::new();
        let srcs: Vec<String> = self.sources.iter().enumerate().map(|(i, s)| format!("{s} {} rows in {:.1} ms", self.source_rows[i], self.source_ms[i])).collect();
        l.push(format!("blend {}@{}: {} sources, {} rows ({})", self.name, self.version, self.sources.len(), self.cands.len(), srcs.join(", ")));
        let sh = self.shares();
        let placed: Vec<String> = self.sources.iter().enumerate().filter(|(i, _)| sh[*i] > 0).map(|(i, s)| format!("{s} {}", sh[i])).collect();
        l.push(format!("blend placed {} of {} slots in {:.2} ms: {}", self.placed.len(), self.k, self.fill_ms, placed.join(", ")));
        for q in &self.quotas {
            l.push(format!("blend quota {}: {} of the top {}{}", q.rule, q.got, q.top, if q.idle { ", idle (no row of its kind)" } else if q.met { "" } else { ", not met" }));
        }
        let relaxed = self.placed.iter().filter(|p| p.kind == "relaxed").count();
        if relaxed > 0 {
            l.push(format!("blend set a rule aside for {relaxed} slots: no row fit them all"));
        }
        for c in &self.costs {
            l.push(format!("blend rule {} costs {:.1}% of the slate's utility, moving {} rows", c.rule, c.share * 100.0, c.moved));
        }
        l
    }

    /// Why each row is where it is, as JSON: the rows with their heads'
    /// parts, the rows passed over, the quotas and the rules' costs.
    pub fn receipts(&self) -> Json {
        let row = |c: &Cand| -> Json {
            let mut cells = serde_json::Map::new();
            for col in &self.columns {
                let v = self.col_of[c.source].get(&col.to_ascii_lowercase()).map_or(Value::Null, |&i| c.row[i].clone());
                if !matches!(v, Value::Vector(_)) {
                    cells.insert(col.clone(), value_json(&v));
                }
            }
            json!({
                "source": self.sources[c.source],
                "rank": c.rank,
                "key": value_json(&c.key),
                "utility": round(c.utility),
                "boost": round(c.boost),
                "parts": self.heads.iter().enumerate().map(|(h, hd)| (hd.name.clone(), json!(round(c.parts[h])))).collect::<serde_json::Map<String, Json>>(),
                "raw": self.heads.iter().enumerate().map(|(h, hd)| (hd.name.clone(), c.raw[h].map_or(Json::Null, |x| json!(round(x))))).collect::<serde_json::Map<String, Json>>(),
                "also": c.also.iter().map(|&s| self.sources[s].clone()).collect::<Vec<_>>(),
                "cells": cells,
            })
        };
        let rows: Vec<Json> = self
            .placed
            .iter()
            .map(|p| {
                let mut j = row(&self.cands[p.cand]);
                j["slot"] = json!(p.slot);
                j["adjusted"] = json!(round(p.adjusted));
                j["kind"] = json!(p.kind);
                j["why"] = json!(p.why);
                j
            })
            .collect();
        let passed: Vec<Json> = self
            .passed
            .iter()
            .map(|(c, why)| {
                let mut j = row(&self.cands[*c]);
                j["why"] = json!(why);
                j
            })
            .collect();
        let sh = self.shares();
        let vectors: Vec<&String> = self.columns.iter().filter(|col| self.cands.iter().any(|c| self.col_of[c.source].get(&col.to_ascii_lowercase()).is_some_and(|&i| matches!(c.row[i], Value::Vector(_))))).collect();
        json!({
            "blend": self.name,
            "version": self.version,
            "k": self.k,
            "columns": self.columns,
            "vectors": vectors,
            "heads": self.heads.iter().map(|h| json!({"name": h.name, "weight": h.weight, "calibrate": h.calibrate.name(), "missing": match h.missing { Missing::Neutral => "neutral", Missing::Zero => "zero" }})).collect::<Vec<_>>(),
            "sources": self.sources.iter().enumerate().map(|(i, s)| json!({"name": s, "rows": self.source_rows[i], "took_ms": round(self.source_ms[i]), "placed": sh[i]})).collect::<Vec<_>>(),
            "rows": rows,
            "passed": passed,
            "quotas": self.quotas.iter().map(|q| json!({"rule": q.rule, "top": q.top, "got": q.got, "least": q.least, "most": q.most, "idle": q.idle, "met": q.met})).collect::<Vec<_>>(),
            "costs": self.costs.iter().map(|c| json!({"rule": c.rule, "share": c.share, "moved": c.moved})).collect::<Vec<_>>(),
            "fill_ms": round(self.fill_ms),
            "plan": self.lines(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};

    fn rs(cols: &[&str], rows: Vec<Vec<Value>>) -> ResultSet {
        let n = rows.len();
        ResultSet { columns: cols.iter().map(|c| c.to_string()).collect(), keys: rows.iter().map(|r| r[0].clone()).collect(), scores: (0..n).map(|i| (n - i) as f64).collect(), rows, parts: None, stats: Default::default() }
    }

    fn s(x: &str) -> Value {
        Value::Str(x.into())
    }

    fn two() -> Def {
        let st = parse_statement(
            "CREATE OR REPLACE BLEND feed AS
               SOURCES (
                 a AS (SELECT id, author, rel FROM t ORDER BY bm25(body, :q) DESC LIMIT 10),
                 b AS (SELECT id, author, rel FROM u WHERE kind = 'x' ORDER BY rel DESC LIMIT 10)
               )
               HEADS (rel CALIBRATE tmm)
               UTILITY 1 * rel
               WITH (k => 6);",
        )
        .unwrap()
        .unwrap();
        let Statement::Create { def, replace } = st else { panic!() };
        assert!(replace);
        def
    }

    fn inputs() -> Vec<Input> {
        let a = rs(&["id", "author", "rel"], (0..10).map(|i| vec![s(&format!("a{i}")), s(if i % 2 == 0 { "ann" } else { "bob" }), Value::Float(10.0 - i as f64)]).collect());
        let b = rs(&["id", "author", "rel"], (0..10).map(|i| vec![s(&format!("b{i}")), s("cy"), Value::Float(5.0 - i as f64 * 0.4)]).collect());
        vec![Input { rs: a, took_ms: 1.0 }, Input { rs: b, took_ms: 1.0 }]
    }

    fn slate(b: &Blended) -> Vec<String> {
        b.placed.iter().map(|p| text(&b.cands[p.cand].key)).collect()
    }

    #[test]
    fn parses_and_reads_back() {
        let d = two();
        assert_eq!(d.sources.len(), 2);
        assert_eq!(d.sources[1].sql, "SELECT id, author, rel FROM u WHERE kind = 'x' ORDER BY rel DESC LIMIT 10");
        assert_eq!(d.k, 6);
        let full = parse_statement(
            "CREATE BLEND serp AS SOURCES (s AS (SELECT id FROM t ORDER BY x DESC LIMIT 5), c AS (SELECT id FROM c LIMIT 5))
             HEADS (relevance CALIBRATE tmm LO -1, engagement CALIBRATE raw MISSING zero, quality CALIBRATE rank)
             UTILITY 1 * relevance + 0.55 * engagement - 0.1 * quality
             BOOST (c -0.06)
             SLOT 1 => source = 's'
             SLOT 3 => kind = 'ask'
             QUOTA (source = 'c' BETWEEN 20% AND 40% IN TOP 10, source = 's' AT LEAST 10% IF ANY, lang = 'en' AT MOST 50%)
             SPACING (source = 'c' AT MOST 1 PER 3 ROWS)
             DIVERSIFY 0.3 ON emb
             MAX 2 PER author
             MAX 1 PER site
             DEDUPE 0.95 ON emb
             WITH (k => 10, note => 'it''s the first')",
        )
        .unwrap()
        .unwrap();
        let Statement::Create { def, replace } = full else { panic!() };
        assert!(!replace);
        assert_eq!(def.heads.iter().map(|h| (h.name.as_str(), h.weight)).collect::<Vec<_>>(), [("relevance", 1.0), ("engagement", 0.55), ("quality", -0.1)]);
        assert_eq!(def.heads[0].lo, Some(-1.0));
        assert_eq!(def.heads[1].missing, Missing::Zero);
        assert_eq!(def.quotas[1].if_any, true);
        assert_eq!(def.quotas[2].max, 50.0);
        assert_eq!(def.caps.len(), 2);
        assert_eq!(def.note.as_deref(), Some("it's the first"));
        // The text it writes reads back as itself, and so does its body.
        let again = parse_statement(&def.sql(false)).unwrap().unwrap();
        assert_eq!(again, Statement::Create { def: def.clone(), replace: false });
        let f = def.function_sql(true);
        assert!(f.starts_with("CREATE OR REPLACE FUNCTION serp() RETURNS TEXT"), "{f}");
        assert_eq!(Def::of_body(f.split(" AS ").last().unwrap()), Some(def));
        assert_eq!(parse_statement("DROP BLEND IF EXISTS serp;").unwrap(), Some(Statement::Drop { name: "serp".into(), if_exists: true }));
        assert_eq!(parse_statement("CREATE TABLE blends (id INT)").unwrap(), None);
        assert_eq!(parse_statement("SELECT * FROM blend('x')").unwrap(), None);
    }

    #[test]
    fn refuses_what_does_not_hold() {
        let e = |sql: &str| parse_statement(sql).unwrap_err().to_string();
        let src = "SOURCES (a AS (SELECT id FROM t LIMIT 5), b AS (SELECT id FROM u LIMIT 5))";
        assert!(e(&format!("CREATE BLEND x AS {src} QUOTA (source = 'a' AT LEAST 60%, source = 'b' AT LEAST 60%)")).contains("ask for 12 of the top 10"));
        assert!(e(&format!("CREATE BLEND x AS {src} QUOTA (source = 'z' AT MOST 10%)")).contains("source z"));
        assert!(e(&format!("CREATE BLEND x AS {src} SLOT 1 => source = 'a' SLOT 2 => source = 'a' QUOTA (source = 'a' AT MOST 10%)")).contains("allows 1"));
        assert!(e("CREATE BLEND x AS SOURCES (a AS (DELETE FROM t))").contains("source a"));
        assert!(e("CREATE BLEND x AS SOURCES (a AS (SELECT * FROM blend('y')))").contains("blend(...)"));
        assert!(e(&format!("CREATE BLEND x AS {src} SPACING (source = 'a' AT MOST 2 PER 2 ROWS)")).contains("SPACING"));
        assert!(e(&format!("CREATE BLEND x AS {src} WITH (k => 0)")).contains("k is 1"));
        assert!(e(&format!("CREATE BLEND x AS {src} HEADS (r) HEADS (s)")).contains("twice"));
        assert!(e("CREATE BLEND x AS HEADS (r)").contains("SOURCES"));
    }

    #[test]
    fn calls_read_their_arguments() {
        let q = crate::query::sql::parse_query("SELECT slot, source, id AS item FROM blend('Feed@3', q => :q, n => 5) LIMIT 4").unwrap();
        let (call, cols) = table_call(&q, &Params::new().set("q", s("rust"))).unwrap().unwrap();
        assert_eq!((call.name.as_str(), call.version, call.limit), ("feed", Some(3), Some(4)));
        assert_eq!(call.params.0.get("q"), Some(&s("rust")));
        assert_eq!(call.params.0.get("n"), Some(&Value::Int(5)));
        assert_eq!(cols[2], ("id".to_string(), "item".to_string()));
        let w = crate::query::sql::parse_query("SELECT * FROM blend('feed') WHERE slot = 1").unwrap();
        assert!(table_call(&w, &Params::new()).unwrap_err().to_string().contains("no join, WHERE"));
        assert_eq!(names_sql("SELECT * FROM blend('feed@2')"), vec!["feed".to_string()]);
        assert!(names_sql("SELECT * FROM t").is_empty());
    }

    #[test]
    fn blends_by_utility_and_keeps_one_row_per_key() {
        let d = two();
        let mut inp = inputs();
        // b0 is a0 too, worth less there.
        inp[1].rs.rows[0][0] = s("a0");
        inp[1].rs.keys[0] = s("a0");
        let b = blend(&d, 1, 6, inp, Want::default()).unwrap();
        // tmm: a's 1.0, 0.9, 0.8...; b's 1.0 (its row a0, kept from a,
        // first), 0.92, 0.84...
        assert_eq!(slate(&b), ["a0", "b1", "a1", "b2", "a2", "b3"]);
        assert_eq!(b.cands[b.placed[0].cand].also, vec![1]);
        let rs = b.result(&[("*".into(), "*".into())]).unwrap();
        assert_eq!(rs.columns, ["slot", "source", "id", "author", "rel", "utility", "blend_decision"]);
        assert_eq!((&rs.rows[1][1], &rs.rows[1][2]), (&s("b"), &s("b1")));
        assert!(rs.stats.plan[0].starts_with("blend feed@1: 2 sources"), "{:?}", rs.stats.plan);
    }

    #[test]
    fn pins_quotas_spacing_caps_and_diversity() {
        let mut d = two();
        d.slots.push(Slot { slot: 1, when: Pred { column: "source".into(), value: json!("b") } });
        let b = blend(&d, 1, 6, inputs(), Want::default()).unwrap();
        assert_eq!(slate(&b)[0], "b0");
        assert_eq!(b.placed[0].kind, "pin");
        // At least half of the top 6 from b: b's rows come in once they must.
        d.slots.clear();
        d.quotas.push(Quota { when: Pred { column: "source".into(), value: json!("b") }, min: 50.0, max: 100.0, top: None, if_any: false });
        let b = blend(&d, 1, 6, inputs(), Want { costs: true }).unwrap();
        assert_eq!(slate(&b).iter().filter(|k| k.starts_with('b')).count(), 3);
        assert!(b.quotas[0].met);
        assert_eq!(b.costs.len(), 1);
        assert!(b.costs[0].share >= 0.0);
        // At most 1 of b in any 3 rows.
        d.quotas.clear();
        d.spacing.push(Spacing { when: Pred { column: "source".into(), value: json!("b") }, at_most: 1, per: 3 });
        let b = blend(&d, 1, 6, inputs(), Want::default()).unwrap();
        let src: Vec<String> = slate(&b).iter().map(|k| k[..1].to_string()).collect();
        for w in src.windows(3) {
            assert!(w.iter().filter(|x| *x == "b").count() <= 1, "{src:?}");
        }
        // At most 1 per author: ann, cy and bob each once (a0 and b0 tie,
        // in the sources' order), then a rule set aside.
        d.spacing.clear();
        d.caps.push(Cap { at_most: 1, per: "author".into() });
        let b = blend(&d, 1, 6, inputs(), Want::default()).unwrap();
        assert_eq!(&slate(&b)[..3], ["a0", "b0", "a1"]);
        assert_eq!(b.placed[3].kind, "relaxed");
        // Over 3 slots, cy's b1 (0.92) is passed over for bob's a1 (0.9).
        let b = blend(&d, 1, 3, inputs(), Want::default()).unwrap();
        assert_eq!(b.passed.iter().map(|(c, why)| (text(&b.cands[*c].key), why.as_str())).next(), Some(("b1".to_string(), "MAX 1 PER author: cy has them")), "{:?}", b.passed);
        // Diversity on author: a row of an author placed loses λ of the
        // range, so ann's a2 (0.8) waits behind cy's b1 (0.92 less 1.0).
        d.caps.clear();
        d.diversify = Some(Diversify { lambda: 0.5, on: "author".into() });
        let b = blend(&d, 1, 6, inputs(), Want::default()).unwrap();
        assert_eq!(&slate(&b)[..3], ["a0", "b0", "a1"]);
        assert!(b.placed[3].why.contains("diversity") || b.placed.iter().any(|p| p.adjusted < b.cands[p.cand].utility), "{:?}", b.placed);
    }

    #[test]
    fn dedupes_near_copies_by_cosine() {
        let st = parse_statement("CREATE BLEND v AS SOURCES (a AS (SELECT id, emb FROM t LIMIT 5)) DEDUPE 0.9 ON emb WITH (k => 3)").unwrap().unwrap();
        let Statement::Create { def, .. } = st else { panic!() };
        let v = |x: f32, y: f32| Value::Vector(vec![x, y].into());
        let r = rs(&["id", "emb"], vec![vec![s("p"), v(1.0, 0.0)], vec![s("q"), v(0.99, 0.05)], vec![s("r"), v(0.0, 1.0)], vec![s("t"), v(0.7, 0.7)]]);
        let b = blend(&def, 1, 3, vec![Input { rs: r, took_ms: 0.0 }], Want::default()).unwrap();
        assert_eq!(slate(&b), ["p", "r", "t"]);
        assert!(b.passed[0].1.contains("near copy of slot 1"), "{:?}", b.passed);
    }

    /// Over random blends: feasible quotas are met, pins kept, caps and
    /// spacing hold when nothing was set aside, and a run is the same
    /// twice.
    #[test]
    fn diversity_and_near_copies_match_a_plain_greedy_fill() {
        // The fill compares a row with the rows placed only when a slot
        // looks at it; over random vectors its slate is the plain greedy
        // one's, which compares every row after each placement.
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        for round in 0..200 {
            let lambda = [0.0, 0.2, 0.6, 1.0][round % 4];
            let t = [0.8, 0.9, 0.99][round % 3];
            let st = parse_statement(&format!("CREATE BLEND v AS SOURCES (a AS (SELECT id, emb, rel FROM t LIMIT 5)) HEADS (rel CALIBRATE raw) DIVERSIFY {lambda} ON emb DEDUPE {t} ON emb WITH (k => 8)")).unwrap().unwrap();
            let Statement::Create { def, .. } = st else { panic!() };
            let n = rng.gen_range(1..40);
            let rows: Vec<Vec<Value>> = (0..n)
                .map(|i| {
                    // Few directions, so that near copies happen.
                    let base = rng.gen_range(0..5) as f32;
                    let v: Vec<f32> = (0..4).map(|j| ((base + j as f32) * 1.3).sin() + rng.gen_range(-0.1..0.1)).collect();
                    vec![s(&format!("r{i}")), Value::Vector(v.into()), Value::Float(rng.gen_range(0.0..1.0))]
                })
                .collect();
            let b = blend(&def, 1, 8, vec![Input { rs: rs(&["id", "emb", "rel"], rows.clone()), took_ms: 0.0 }], Want::default()).unwrap();
            // The plain greedy fill over the same rows, in the same order.
            let unitv = |r: &Vec<Value>| unit(r[1].as_vector().unwrap()).unwrap();
            let util: Vec<f64> = b.cands.iter().map(|c| c.utility).collect();
            let vecs: Vec<Vec<f32>> = b.cands.iter().map(|c| unitv(&c.row)).collect();
            let mut order: Vec<usize> = (0..b.cands.len()).collect();
            order.sort_by(|&x, &y| util[y].total_cmp(&util[x]).then(b.cands[x].rank.cmp(&b.cands[y].rank)));
            let (lo, hi) = util.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), &u| (a.min(u), z.max(u)));
            let scale = if hi - lo > 1e-12 { hi - lo } else { 1.0 };
            let (mut placed, mut gone) = (Vec::<usize>::new(), vec![false; b.cands.len()]);
            while placed.len() < 8 {
                let mut best: Option<(usize, f64)> = None;
                for &c in &order {
                    if gone[c] || placed.contains(&c) {
                        continue;
                    }
                    let most = placed.iter().map(|&p| cosine(&vecs[c], &vecs[p]).max(0.0)).fold(0.0, f64::max);
                    let adj = util[c] - lambda * scale * most;
                    if best.is_none_or(|bb| adj > bb.1) {
                        best = Some((c, adj));
                    }
                }
                let Some((c, _)) = best else { break };
                placed.push(c);
                for o in 0..b.cands.len() {
                    if !placed.contains(&o) && cosine(&vecs[c], &vecs[o]) >= t {
                        gone[o] = true;
                    }
                }
            }
            let want: Vec<usize> = placed;
            let got: Vec<usize> = b.placed.iter().map(|p| p.cand).collect();
            assert_eq!(got, want, "round {round}: λ {lambda}, threshold {t}");
            for (i, &x) in got.iter().enumerate() {
                for &y in &got[..i] {
                    assert!(cosine(&vecs[x], &vecs[y]) < t, "round {round}: a near copy placed");
                }
            }
        }
    }

    #[test]
    fn random_fills_keep_their_rules() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        for round in 0..300 {
            let n_src = rng.gen_range(1..=4);
            let names: Vec<String> = (0..n_src).map(|i| format!("s{i}")).collect();
            let sources = names.iter().map(|n| Source { name: n.clone(), sql: "SELECT id, author, rel FROM t LIMIT 5".into() }).collect();
            let k = rng.gen_range(1..=12);
            let mut d = Def { name: "r".into(), sources, heads: vec![Head { name: "rel".into(), weight: 1.0, calibrate: [Calibrate::Tmm, Calibrate::Minmax, Calibrate::Rank, Calibrate::Raw][rng.gen_range(0..4)], lo: None, missing: Missing::Neutral }], boost: Vec::new(), slots: Vec::new(), quotas: Vec::new(), spacing: Vec::new(), diversify: None, caps: Vec::new(), dedupe: None, k, note: None };
            if rng.gen_bool(0.5) {
                d.slots.push(Slot { slot: rng.gen_range(1..=k), when: Pred { column: "source".into(), value: json!(names[rng.gen_range(0..n_src)]) } });
            }
            for nm in &names {
                if rng.gen_bool(0.5) {
                    let min = rng.gen_range(0..=40) as f64;
                    d.quotas.push(Quota { when: Pred { column: "source".into(), value: json!(nm) }, min, max: (min + rng.gen_range(0..=60) as f64).min(100.0), top: rng.gen_bool(0.5).then(|| rng.gen_range(1..=k)), if_any: false });
                }
            }
            if rng.gen_bool(0.3) {
                d.caps.push(Cap { at_most: rng.gen_range(1..=3), per: "author".into() });
            }
            if rng.gen_bool(0.3) {
                d.diversify = Some(Diversify { lambda: rng.gen_range(0.0..1.0), on: "author".into() });
            }
            if d.feasible(k).is_err() {
                continue;
            }
            let mk = |rng: &mut rand::rngs::StdRng| -> Vec<Input> {
                (0..n_src)
                    .map(|si| {
                        let rows = (0..rng.gen_range(0..15)).map(|i| vec![s(&format!("{si}-{i}")), s(&format!("au{}", rng.gen_range(0..4))), Value::Float(rng.gen_range(0.0..10.0))]).collect();
                        Input { rs: rs(&["id", "author", "rel"], rows), took_ms: 0.0 }
                    })
                    .collect()
            };
            let mut r2 = rand::rngs::StdRng::seed_from_u64(round);
            let b = blend(&d, 1, k, mk(&mut r2), Want::default()).unwrap();
            let mut r3 = rand::rngs::StdRng::seed_from_u64(round);
            let again = blend(&d, 1, k, mk(&mut r3), Want::default()).unwrap();
            assert_eq!(slate(&b), slate(&again), "round {round}");
            let src_of = |p: &Placed| b.sources[b.cands[p.cand].source].clone();
            let any_relaxed = b.placed.iter().any(|p| p.kind == "relaxed");
            let avail: Vec<usize> = (0..n_src).map(|i| b.cands.iter().filter(|c| c.source == i).count()).collect();
            let filled = b.placed.len() == k.min(b.cands.len());
            assert!(filled, "round {round}: {} of {}", b.placed.len(), k);
            for sl in &d.slots {
                let want = json_text(&sl.when.value);
                let si = d.source(&want).unwrap();
                if let Some(p) = b.placed.iter().find(|p| p.slot == sl.slot) {
                    if !any_relaxed && avail[si] > 0 {
                        assert_eq!(src_of(p), want, "round {round}: slot {}", sl.slot);
                    }
                }
            }
            if !any_relaxed {
                for q in &b.quotas {
                    assert!(q.got <= q.most, "round {round}: {q:?}");
                }
                for (q, quota) in d.quotas.iter().enumerate() {
                    let si = d.source(&json_text(&quota.when.value)).unwrap();
                    // Enough rows of its kind and of others: met.
                    if avail[si] >= b.quotas[q].least && b.cands.len() >= k + b.quotas[q].most {
                        assert!(b.quotas[q].got >= b.quotas[q].least, "round {round}: {:?}", b.quotas[q]);
                    }
                }
                for c in &d.caps {
                    let mut per: HashMap<String, usize> = HashMap::new();
                    for p in &b.placed {
                        *per.entry(text(&b.cell(p, &c.per))).or_insert(0) += 1;
                    }
                    assert!(per.values().all(|&n| n <= c.at_most), "round {round}");
                }
            }
        }
    }

    /// The fill's time over many rows (`cargo test --release -- --ignored
    /// fill_time --nocapture`).
    #[test]
    #[ignore]
    fn fill_time() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for n in [100usize, 500, 2000] {
            let st = parse_statement(
                "CREATE BLEND t AS SOURCES (a AS (SELECT id FROM t LIMIT 5), b AS (SELECT id FROM t LIMIT 5), c AS (SELECT id FROM t LIMIT 5), d AS (SELECT id FROM t LIMIT 5))
                 HEADS (rel, eng CALIBRATE minmax) UTILITY 1 * rel + 0.5 * eng
                 SLOT 1 => source = 'a' QUOTA (source = 'b' BETWEEN 20% AND 40%, source = 'c' AT MOST 10%)
                 SPACING (source = 'd' AT MOST 1 PER 4 ROWS) DIVERSIFY 0.3 ON emb MAX 2 PER author DEDUPE 0.97 ON emb WITH (k => 20)",
            )
            .unwrap()
            .unwrap();
            let Statement::Create { def, .. } = st else { panic!() };
            let mk = |rng: &mut rand::rngs::StdRng| -> Vec<Input> {
                (0..4)
                    .map(|si| {
                        let rows = (0..n / 4)
                            .map(|i| {
                                let v: Vec<f32> = (0..256).map(|_| rng.gen_range(-1.0..1.0)).collect();
                                vec![s(&format!("{si}-{i}")), s(&format!("au{}", rng.gen_range(0..50))), Value::Vector(v.into()), Value::Float(rng.gen_range(0.0..10.0)), Value::Float(rng.gen_range(0.0..1.0))]
                            })
                            .collect();
                        Input { rs: rs(&["id", "author", "emb", "rel", "eng"], rows), took_ms: 0.0 }
                    })
                    .collect()
            };
            let runs = 20;
            let mut ms = Vec::new();
            for _ in 0..runs {
                let inp = mk(&mut rng);
                let t = std::time::Instant::now();
                let b = blend(&def, 1, 20, inp, Want::default()).unwrap();
                ms.push(t.elapsed().as_secs_f64() * 1e3);
                assert_eq!(b.placed.len(), 20);
            }
            ms.sort_by(f64::total_cmp);
            println!("fill of {n} rows into 20 slots, 256-d vectors, every rule: median {:.3} ms, max {:.3} ms", ms[runs / 2], ms[runs - 1]);
        }
    }
}
