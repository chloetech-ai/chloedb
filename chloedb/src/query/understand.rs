//! Query understanding: the words of a query that name values of the
//! table's `KEYWORD` fields, or a range of a number, become conditions
//! (`bm25(name, :q, understand => 'boost' | 'filter')`).
//!
//! ```sql
//! -- "white modern coffee table under 300": color, style and class are
//! -- KEYWORD fields, price a FLOAT
//! SELECT id FROM products
//! ORDER BY bm25(body, :q, understand => 'boost', understand_fields => 'color, style, class, price') DESC
//! LIMIT 10;
//! ```
//!
//! A field's values are its *dictionary*, read by an ordinary query
//! (`SELECT color, COUNT(*) FROM products GROUP BY color`) and analysed as
//! the text field is, so `Coffee & Cocktail Tables` is `coffe cocktail
//! tabl`. Words of the query match a value when they are a run of its terms
//! that holds its last one, its head: `coffee table` names `Coffee &
//! Cocktail Tables` and `Coffee Tables`, `chair` every value that ends in
//! chairs. A value named in full leaves out the field's partial matches of
//! the same words. Matches of several fields that share words are one
//! condition, any of them (`color = 'white' OR finish = 'white'`), and a
//! number after `under`, `over`, `between`, `less than`, ... a range of a
//! numeric field listed in `understand_fields`.
//!
//! - **`boost`** ranks by `bm25(..) * (1 + w * c)`, where `c` counts the
//!   conditions a row meets and `w` is `understand_weight`
//!   ([`DEFAULT_WEIGHT`]): a [rank-safe boost], exact over every row the
//!   text matches.
//! - **`filter`** adds the conditions of values named in full and of
//!   ranges to the query's `WHERE`, and boosts by the others.
//! - **`off`** is the call as written, without the options.
//!
//! In both modes the words of a range leave the ranked text: `under 300`
//! is not text to find. The rewrite is shown in the answer's plan lines.
//!
//! Without `understand_fields`, every `KEYWORD` field of the table but its
//! key is read, and a field of more than [`MAX_VALUES`] values is left out
//! as a name rather than a facet.
//!
//! [rank-safe boost]: https://github.com/chloetech-ai/chloedb/blob/main/docs/chloedb/query-language.md#rank-safe-boosts

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;

use serde::Serialize;
use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, VisitMut, VisitorMut};

use crate::error::{Error, Result};
use crate::query::exec::{words_of, ResultSet};
use crate::query::{compound, Params};
use crate::schema::{FieldType, Schema};
use crate::text::Analyzer;
use crate::value::Value;

/// The option that turns understanding on: its mode.
pub const UNDERSTAND: &str = "understand";
/// The fields to read, a comma-separated list.
pub const UNDERSTAND_FIELDS: &str = "understand_fields";
/// The weight of a condition a row meets in a boost.
pub const UNDERSTAND_WEIGHT: &str = "understand_weight";
/// The modes, as `understand =>` takes them.
pub const MODES: [&str; 3] = ["off", "boost", "filter"];
/// A boost's weight unless `understand_weight` says.
pub const DEFAULT_WEIGHT: f64 = 0.25;
/// The least share a run's values need: of the rows whose text holds the
/// words, those that hold one of the values ([`Understood::confirm`]).
pub const UNDERSTAND_SHARE: &str = "understand_share";
/// [`UNDERSTAND_SHARE`] unless the call says.
pub const DEFAULT_SHARE: f64 = 0.05;
/// A field with more values than this is not read unless listed.
pub const MAX_VALUES: usize = 20_000;
/// The most values of a listed field read, most rows first.
pub const MAX_LISTED_VALUES: usize = 200_000;
/// The values of one field one run of words may name, most rows first.
pub const MAX_MATCHES: usize = 50;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// Whether the text of a query may understand: a test that costs nothing,
/// before [`plan`] looks.
pub fn may_understand(sql: &str) -> bool {
    let b = sql.as_bytes();
    b.windows(UNDERSTAND.len()).any(|w| w.eq_ignore_ascii_case(UNDERSTAND.as_bytes()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Off,
    Boost,
    Filter,
}

impl Mode {
    pub fn of(s: &str) -> Result<Mode> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Ok(Mode::Off),
            "boost" => Ok(Mode::Boost),
            "filter" => Ok(Mode::Filter),
            _ => Err(err(format!("understand is one of {}, not '{s}'", MODES.map(|m| format!("'{m}'")).join(", ")))),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Boost => "boost",
            Mode::Filter => "filter",
        }
    }
}

/// How a call understands: [`Mode`], fields, weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub mode: Mode,
    /// The fields named, as written; `None` for the table's own choice.
    pub fields: Option<Vec<String>>,
    pub weight: f64,
    /// [`UNDERSTAND_SHARE`].
    pub share: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { mode: Mode::Boost, fields: None, weight: DEFAULT_WEIGHT, share: DEFAULT_SHARE }
    }
}

/// The fields understanding reads for a text field of `schema`: the
/// keyword fields (whose dictionaries the caller reads, those of more than
/// [`MAX_VALUES`] values skipped unless `listed`) and the numeric ones.
#[derive(Clone, Debug, PartialEq)]
pub struct Fields {
    pub keywords: Vec<String>,
    pub numbers: Vec<String>,
    /// Whether the fields were listed rather than chosen.
    pub listed: bool,
}

pub fn fields(schema: &Schema, text_field: &str, listed: Option<&[String]>) -> Result<Fields> {
    let tf = schema.field(text_field).ok_or_else(|| err(format!("{} has no field {text_field}", schema.table)))?;
    if !matches!(tf.ty, FieldType::Text { .. }) {
        return Err(err(format!("understand reads the words of a TEXT field; {text_field} is not one")));
    }
    let (mut keywords, mut numbers) = (Vec::new(), Vec::new());
    match listed {
        Some(names) => {
            for n in names {
                let f = schema.field(n).ok_or_else(|| err(format!("understand_fields: {} has no field {n}", schema.table)))?;
                match f.ty {
                    FieldType::Keyword => keywords.push(f.name.clone()),
                    FieldType::Int | FieldType::Float => numbers.push(f.name.clone()),
                    _ => return Err(err(format!("understand_fields names KEYWORD fields, whose values the words may name, and INT or FLOAT ones, for ranges: {n} is neither"))),
                }
            }
        }
        None => {
            for (i, f) in schema.fields.iter().enumerate() {
                if matches!(f.ty, FieldType::Keyword) && Some(i) != schema.primary_key && f.indexed && f.semantic_id.is_none() {
                    keywords.push(f.name.clone());
                }
            }
        }
    }
    Ok(Fields { keywords, numbers, listed: listed.is_some() })
}

/// The query that reads a field's dictionary: its values and their rows,
/// most rows first, one more than [`MAX_VALUES`] to tell when it was cut.
pub fn dictionary_sql(table: &str, field: &str) -> String {
    format!("SELECT {f}, COUNT(*) FROM {t} GROUP BY {f} ORDER BY COUNT(*) DESC LIMIT {MAX_LISTED_VALUES}", f = Ident::new(field), t = Ident::new(table))
}

#[derive(Clone, Debug)]
struct Entry {
    terms: Vec<String>,
    value: String,
    rows: u64,
}

/// A keyword field's values analysed as a text field is, by their terms.
#[derive(Clone, Debug, Default)]
pub struct Gazetteer {
    pub field: String,
    /// Whether the field has more values than [`MAX_VALUES`].
    pub cut: bool,
    pub values: usize,
    /// Entries by each of their terms.
    by_term: HashMap<String, Vec<u32>>,
    entries: Vec<Entry>,
}

impl Gazetteer {
    /// The gazetteer of `field` from the answer of its [`dictionary_sql`].
    pub fn build(field: &str, rows: &ResultSet, a: Analyzer) -> Gazetteer {
        let mut g = Gazetteer { field: field.to_string(), cut: rows.rows.len() > MAX_VALUES, ..Default::default() };
        for r in &rows.rows {
            let (Some(Value::Str(v)), Some(n)) = (r.first(), r.get(1).and_then(|n| n.as_i64())) else { continue };
            g.add(v, n.max(0) as u64, a);
        }
        g
    }

    /// The gazetteer of these values and their rows.
    pub fn of(field: &str, values: &[(&str, u64)], a: Analyzer) -> Gazetteer {
        let mut g = Gazetteer { field: field.to_string(), ..Default::default() };
        for (v, n) in values {
            g.add(v, *n, a);
        }
        g
    }

    fn add(&mut self, v: &str, rows: u64, a: Analyzer) {
        self.values += 1;
        let terms = a.query_term_seq(v, false);
        // A value of numbers alone (`2`, `100 x 50`) names nothing a query's
        // words say.
        if terms.is_empty() || terms.iter().all(|t| t.chars().all(|c| c.is_ascii_digit())) {
            return;
        }
        let i = self.entries.len() as u32;
        let mut seen: Vec<&String> = Vec::new();
        for t in &terms {
            if !seen.contains(&t) {
                self.by_term.entry(t.clone()).or_default().push(i);
                seen.push(t);
            }
        }
        self.entries.push(Entry { terms, value: v.to_string(), rows });
    }
}

/// A value a run of words names.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Match {
    pub field: String,
    pub value: String,
    /// The rows that hold it.
    pub rows: u64,
    /// Whether the words name it in full, rather than its head.
    pub full: bool,
}

/// A run of the query's words and the values they name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Span {
    /// Byte offsets in the text.
    pub start: usize,
    pub end: usize,
    pub words: String,
    pub matches: Vec<Match>,
    /// Whether a `filter` filters by it: some value is named in full.
    pub filters: bool,
    /// Of the rows whose text holds the words, the share that hold one of
    /// the values ([`Understood::confirm`]); `None` before it is counted.
    pub share: Option<f64>,
}

/// A range of a numeric field the words say.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Range {
    pub start: usize,
    pub end: usize,
    pub words: String,
    pub field: String,
    /// The bounds, and whether each holds its own value.
    pub lo: Option<(f64, bool)>,
    pub hi: Option<(f64, bool)>,
}

/// What a text says, for a table's fields.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Understood {
    pub table: String,
    pub field: String,
    pub mode: Mode,
    pub weight: f64,
    /// The text as given.
    pub text: String,
    /// The text ranked by: the given one without the words of ranges.
    pub ranked: String,
    pub spans: Vec<Span>,
    pub ranges: Vec<Range>,
    /// The keyword fields read, and those left out for their many values.
    pub read: Vec<String>,
    pub skipped: Vec<String>,
    /// The least share a run's values need, and the runs left out for less.
    pub min_share: f64,
    pub dropped: Vec<Span>,
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

impl Span {
    /// The condition a `filter` puts in the `WHERE`: a row holds one of
    /// the values named in full, or none of their fields has a value.
    pub fn filter(&self) -> String {
        let full: Vec<&Match> = self.matches.iter().filter(|m| m.full).collect();
        let mut fields: Vec<&str> = Vec::new();
        for m in &full {
            if !fields.contains(&m.field.as_str()) {
                fields.push(&m.field);
            }
        }
        let none: Vec<String> = fields.iter().map(|f| format!("{} IS NULL", Ident::new(*f))).collect();
        let parts = condition_of(full.into_iter());
        format!("({} OR {})", parts.join(" OR "), none.join(" AND "))
    }

    /// The condition a row meets when it holds one of the values.
    pub fn condition(&self) -> String {
        let parts = condition_of(self.matches.iter());
        if parts.len() == 1 {
            parts.into_iter().next().unwrap()
        } else {
            format!("({})", parts.join(" OR "))
        }
    }
}

/// `field IN (values)` for each field of `ms`.
fn condition_of<'a>(ms: impl Iterator<Item = &'a Match>) -> Vec<String> {
    let mut by_field: Vec<(&str, Vec<&str>)> = Vec::new();
    for m in ms {
        match by_field.iter_mut().find(|(f, _)| *f == m.field) {
            Some((_, vs)) => vs.push(&m.value),
            None => by_field.push((&m.field, vec![&m.value])),
        }
    }
    by_field
        .iter()
        .map(|(f, vs)| {
            let f = Ident::new(*f).to_string();
            if vs.len() == 1 {
                format!("{f} = {}", quote(vs[0]))
            } else {
                format!("{f} IN ({})", vs.iter().map(|v| quote(v)).collect::<Vec<_>>().join(", "))
            }
        })
        .collect()
}

impl Range {
    pub fn condition(&self) -> String {
        let f = Ident::new(&self.field).to_string();
        let mut parts = Vec::new();
        if let Some((x, inc)) = self.lo {
            parts.push(format!("{f} {} {}", if inc { ">=" } else { ">" }, num(x)));
        }
        if let Some((x, inc)) = self.hi {
            parts.push(format!("{f} {} {}", if inc { "<=" } else { "<" }, num(x)));
        }
        parts.join(" AND ")
    }
}

impl Understood {
    /// The counts [`Understood::confirm`] takes: for each run, the rows
    /// whose text holds its words, then those of them that hold one of its
    /// values.
    pub fn checks(&self) -> Vec<String> {
        let t = Ident::new(&self.table);
        let f = Ident::new(&self.field);
        let mut out = Vec::new();
        for s in &self.spans {
            let words: Vec<String> = words_of(&s.words).iter().map(|(_, w)| w.to_lowercase()).collect();
            let m = format!("MATCH({f}, {})", quote(&words.join(" ")));
            out.push(format!("SELECT COUNT(*) FROM {t} WHERE {m}"));
            out.push(format!("SELECT COUNT(*) FROM {t} WHERE {m} AND {}", s.condition()));
        }
        out
    }

    /// Each run's share from `counts` (the answers of
    /// [`Understood::checks`]'s queries, in order); the runs whose share is
    /// below [`Understood::min_share`] are dropped: words that most rows
    /// saying them use otherwise (`up` for a brand named Up).
    pub fn confirm(&mut self, counts: &[u64]) {
        let spans = std::mem::take(&mut self.spans);
        for (i, mut s) in spans.into_iter().enumerate() {
            let (all, held) = (counts.get(2 * i).copied().unwrap_or(0), counts.get(2 * i + 1).copied().unwrap_or(0));
            // Words no row's text holds name what the values say.
            let share = if all == 0 { 1.0 } else { held as f64 / all as f64 };
            s.share = Some(share);
            if share >= self.min_share {
                self.spans.push(s);
            } else {
                self.dropped.push(s);
            }
        }
    }

    /// The conditions a `filter` puts in the `WHERE`.
    pub fn filters(&self) -> Vec<String> {
        if self.mode != Mode::Filter {
            return Vec::new();
        }
        let mut out: Vec<String> = self.spans.iter().filter(|s| s.filters).map(Span::filter).collect();
        out.extend(self.ranges.iter().map(Range::condition));
        out
    }

    /// The conditions that boost.
    pub fn boosts(&self) -> Vec<String> {
        match self.mode {
            Mode::Off => Vec::new(),
            Mode::Boost => self.spans.iter().map(Span::condition).chain(self.ranges.iter().map(Range::condition)).collect(),
            Mode::Filter => self.spans.iter().map(Span::condition).collect(),
        }
    }

    /// The plan line.
    pub fn line(&self) -> String {
        let mut said = Vec::new();
        for c in self.filters() {
            said.push(format!("WHERE {c}"));
        }
        let b = self.boosts();
        if !b.is_empty() {
            said.push(format!("boost {} for each of {}", num(self.weight), b.join(", ")));
        }
        if said.is_empty() {
            said.push("nothing".into());
        }
        let ranked = if self.ranked != self.text { format!(", ranking by '{}'", self.ranked) } else { String::new() };
        format!("bm25({}) understood '{}' with {} of {}: {}{ranked}", self.field, self.text, self.mode.name(), self.table, said.join("; "))
    }
}

/// Words that put a number's bound, and which.
const BELOW: [&[&str]; 9] = [&["under"], &["below"], &["less", "than"], &["cheaper", "than"], &["lower", "than"], &["up", "to"], &["max"], &["maximum"], &["within"]];
const ABOVE: [&[&str]; 7] = [&["over"], &["above"], &["more", "than"], &["greater", "than"], &["at", "least"], &["min"], &["minimum"]];
const AFTER: [&[&str]; 1] = [&["after"]];
const BEFORE: [&[&str]; 1] = [&["before"]];
/// Bounds that hold their own value.
const HOLDS: [&[&str]; 7] = [&["up", "to"], &["max"], &["maximum"], &["within"], &["at", "least"], &["min"], &["minimum"]];

fn number(w: &str) -> Option<f64> {
    let (digits, k) = match w.strip_suffix(['k', 'K']) {
        Some(d) => (d, 1000.0),
        None => (w, 1.0),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<f64>().ok().map(|x| x * k)
}

/// The text's words, lowercased, with their byte offsets; numbers written
/// `1,500` or `12.5` are one word.
fn tokens(text: &str) -> Vec<(usize, usize, String)> {
    let ws = words_of(text);
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    for (o, w) in ws {
        let end = o + w.len();
        if let Some(last) = out.last_mut() {
            let gap = &text[last.1..o];
            if (gap == "," || gap == ".") && last.2.chars().all(|c| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_digit()) {
                last.2 = if gap == "," { format!("{}{w}", last.2) } else { format!("{}.{w}", last.2) };
                last.1 = end;
                continue;
            }
        }
        out.push((o, end, w.to_lowercase()));
    }
    out
}

fn decimal(w: &str) -> Option<f64> {
    match w.split_once('.') {
        Some((a, b)) if !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) => w.parse().ok(),
        _ => number(w),
    }
}

fn at(toks: &[(usize, usize, String)], i: usize, words: &[&str]) -> bool {
    words.iter().enumerate().all(|(j, w)| toks.get(i + j).is_some_and(|t| t.2 == *w))
}

/// The field a range at tokens `i..j` is of: one named by the word before it
/// or after it, else the only one.
fn range_field(toks: &[(usize, usize, String)], i: usize, j: usize, numbers: &[(String, Vec<String>)], a: Analyzer) -> Option<String> {
    let named = |k: Option<usize>| -> Option<String> {
        let t = a.query_term_seq(&toks.get(k?)?.2, false);
        let t = t.first()?;
        numbers.iter().find(|(_, terms)| terms.contains(t)).map(|(f, _)| f.clone())
    };
    // A year (`since 2020`) is of a field named for years.
    let year = || -> Option<String> {
        let x = decimal(&toks.get(j.checked_sub(1)?)?.2)?;
        let year = a.query_term_seq("year", false);
        ((1900.0..=2100.0).contains(&x) && x.fract() == 0.0).then(|| numbers.iter().find(|(_, terms)| terms.iter().any(|t| year.contains(t))).map(|(f, _)| f.clone()))?
    };
    named(Some(j)).or_else(|| named(i.checked_sub(1))).or_else(year).or_else(|| (numbers.len() == 1).then(|| numbers[0].0.clone()))
}

/// The ranges of numeric fields `numbers` the words of `text` say.
pub fn ranges(text: &str, numbers: &[String], a: Analyzer) -> Vec<Range> {
    if numbers.is_empty() {
        return Vec::new();
    }
    let named: Vec<(String, Vec<String>)> = numbers.iter().map(|f| (f.clone(), a.query_term_seq(&f.replace('_', " "), false))).collect();
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let mut found: Option<(usize, Option<(f64, bool)>, Option<(f64, bool)>)> = None;
        // between N and M, from N to M
        if (toks[i].2 == "between" || toks[i].2 == "from") && i + 3 < toks.len() && (toks[i + 2].2 == "and" || toks[i + 2].2 == "to") {
            if let (Some(x), Some(y)) = (decimal(&toks[i + 1].2), decimal(&toks[i + 3].2)) {
                found = Some((i + 4, Some((x.min(y), true)), Some((x.max(y), true))));
            }
        }
        let bounds: [(&[&[&str]], bool); 5] = [(&BELOW, false), (&ABOVE, true), (&[&["since"]], true), (&AFTER, true), (&BEFORE, false)];
        if found.is_none() {
            'b: for (phrases, lo) in bounds {
                for p in phrases {
                    if at(&toks, i, p) {
                        if let Some(x) = toks.get(i + p.len()).and_then(|t| decimal(&t.2)) {
                            // "up to", "at least", "since": the bound holds.
                            let inc = HOLDS.contains(p) || p[0] == "since";
                            found = Some(if lo { (i + p.len() + 1, Some((x, inc)), None) } else { (i + p.len() + 1, None, Some((x, inc))) });
                            break 'b;
                        }
                    }
                }
            }
        }
        match found {
            Some((j, lo, hi)) => {
                // "100 dollars": a unit word after the number is the range's.
                let mut end = j;
                let unit = toks.get(j).is_some_and(|t| ["dollars", "dollar", "usd", "bucks", "eur", "euros"].contains(&t.2.as_str()));
                if unit {
                    end += 1;
                }
                if let Some(field) = range_field(&toks, i, end, &named, a) {
                    let start = toks[i].0;
                    // a "$" before the number is the range's too
                    let stop = toks[end - 1].1;
                    out.push(Range { start, end: stop, words: text[start..stop].to_string(), field, lo, hi });
                }
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// The text without the words of `ranges`, spaces kept single.
fn without(text: &str, ranges: &[Range]) -> String {
    let mut out = String::new();
    let mut at = 0;
    for r in ranges {
        out.push_str(&text[at..r.start]);
        at = r.end;
    }
    out.push_str(&text[at..]);
    out.split_whitespace().filter(|w| *w != "$").collect::<Vec<_>>().join(" ")
}

/// The runs of words of `text` that name values of the gazetteers' fields.
pub fn spans(text: &str, gazetteers: &[&Gazetteer], a: Analyzer, skip: &[Range]) -> Vec<Span> {
    // Each word's terms, with its offsets.
    let mut words: Vec<(usize, usize, String)> = Vec::new();
    for (o, w) in words_of(text) {
        if skip.iter().any(|r| o >= r.start && o < r.end) {
            continue;
        }
        for t in a.query_term_seq(w, false) {
            words.push((o, o + w.len(), t));
        }
    }
    // Every match: (first word, past last word, field, entry, full).
    let mut found: Vec<(usize, usize, usize, u32, bool)> = Vec::new();
    for (gi, g) in gazetteers.iter().enumerate() {
        let mut field_found: Vec<(usize, usize, u32, bool)> = Vec::new();
        for i in 0..words.len() {
            let Some(cands) = g.by_term.get(&words[i].2) else { continue };
            for &e in cands {
                let terms = &g.entries[e as usize].terms;
                // The longest run from i of the value's terms, in its order
                // (`coffee table` in `coffee cocktail tabl`), that ends
                // with its head.
                let Some(p) = terms.iter().position(|t| *t == words[i].2) else { continue };
                let (mut j, mut q, mut n) = (i + 1, p, 1);
                let mut best = (q + 1 == terms.len()).then_some((j, n));
                while j < words.len() {
                    let Some(d) = terms[q + 1..].iter().position(|t| *t == words[j].2) else { break };
                    q += 1 + d;
                    j += 1;
                    n += 1;
                    if q + 1 == terms.len() {
                        best = Some((j, n));
                    }
                }
                let Some((j, n)) = best else { continue };
                let full = n == terms.len();
                field_found.push((i, j, e, full));
            }
        }
        // A field's longest runs first, and every value of the same run.
        field_found.sort_by(|x, y| (y.1 - y.0).cmp(&(x.1 - x.0)).then(y.3.cmp(&x.3)).then(x.0.cmp(&y.0)));
        let mut taken: Vec<(usize, usize, bool)> = Vec::new();
        for (i, j, e, full) in field_found {
            if let Some(t) = taken.iter().find(|t| t.0 < j && i < t.1) {
                // Another value of the same run is kept; one of words of
                // a longer run is not.
                if !(t.0 == i && t.1 == j) {
                    continue;
                }
            } else {
                taken.push((i, j, full));
            }
            found.push((i, j, gi, e, full));
        }
    }
    // A run inside a longer one of another field is that one's words
    // (`coffee` of `coffee table`).
    let all = found.clone();
    found.retain(|f| !all.iter().any(|g| g.1 - g.0 > f.1 - f.0 && g.0 < f.1 && f.0 < g.1));
    // Runs that share words are one condition.
    found.sort_by_key(|f| (f.0, std::cmp::Reverse(f.1)));
    let mut out: Vec<Span> = Vec::new();
    let mut cur: Option<(usize, usize, Vec<(usize, u32, bool)>)> = None;
    let flush = |c: (usize, usize, Vec<(usize, u32, bool)>), out: &mut Vec<Span>| {
        let (i, j, ms) = c;
        let (start, end) = (words[i].0, words[j - 1].1);
        let mut matches: Vec<Match> = ms.iter().map(|&(g, e, full)| {
            let en = &gazetteers[g].entries[e as usize];
            Match { field: gazetteers[g].field.clone(), value: en.value.clone(), rows: en.rows, full }
        }).collect();
        matches.sort_by(|a, b| b.full.cmp(&a.full).then(b.rows.cmp(&a.rows)));
        matches.truncate(MAX_MATCHES);
        let filters = matches.iter().any(|m| m.full);
        out.push(Span { start, end, words: text[start..end].to_string(), matches, filters, share: None });
    };
    for (i, j, g, e, full) in found {
        match &mut cur {
            Some(c) if i < c.1 => {
                c.1 = c.1.max(j);
                c.2.push((g, e, full));
            }
            _ => {
                if let Some(c) = cur.take() {
                    flush(c, &mut out);
                }
                cur = Some((i, j, vec![(g, e, full)]));
            }
        }
    }
    if let Some(c) = cur.take() {
        flush(c, &mut out);
    }
    out
}

/// What `text` says for the fields of `gazetteers` and the numeric
/// `numbers`, analysed by `a`.
pub fn understand(table: &str, field: &str, text: &str, opts: &Options, gazetteers: &[&Gazetteer], numbers: &[String], a: Analyzer) -> Understood {
    let rs = ranges(text, numbers, a);
    let sp = spans(text, gazetteers, a, &rs);
    let ranked = without(text, &rs);
    Understood {
        table: table.to_string(),
        field: field.to_string(),
        mode: opts.mode,
        weight: opts.weight,
        text: text.to_string(),
        ranked: if ranked.is_empty() { text.to_string() } else { ranked },
        spans: sp,
        ranges: rs,
        read: gazetteers.iter().map(|g| g.field.clone()).collect(),
        skipped: Vec::new(),
        min_share: opts.share,
        dropped: Vec::new(),
    }
}

// ---- The rewrite of a query's calls ----

#[derive(Clone, Debug)]
struct Site {
    table: String,
    field: String,
    text: Expr,
    opts: Options,
}

/// A query whose calls understand: the fields each reads, then
/// [`Understanding::finish`].
#[derive(Clone, Debug)]
pub struct Understanding {
    query: sql::Query,
    sites: Vec<Option<Site>>,
}

fn name_is(f: &sql::Function, name: &str) -> bool {
    f.name.to_string().eq_ignore_ascii_case(name)
}

fn args_of(f: &sql::Function) -> Option<(Vec<Expr>, Vec<(String, FunctionArg, Expr)>)> {
    let FunctionArguments::List(l) = &f.args else { return None };
    let (mut pos, mut named) = (Vec::new(), Vec::new());
    for a in &l.args {
        match a {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => pos.push(e.clone()),
            FunctionArg::Named { name, arg: FunctionArgExpr::Expr(e), .. } => named.push((name.value.to_ascii_lowercase(), a.clone(), e.clone())),
            FunctionArg::ExprNamed { name: Expr::Identifier(name), arg: FunctionArgExpr::Expr(e), .. } => named.push((name.value.to_ascii_lowercase(), a.clone(), e.clone())),
            _ => return None,
        }
    }
    Some((pos, named))
}

fn understands(f: &sql::Function) -> bool {
    name_is(f, "bm25") && args_of(f).is_some_and(|(_, named)| named.iter().any(|(n, _, _)| n == UNDERSTAND))
}

const OWN: [&str; 4] = [UNDERSTAND, UNDERSTAND_FIELDS, UNDERSTAND_WEIGHT, UNDERSTAND_SHARE];

fn site(f: &sql::Function, ctx: Option<&String>, params: &Params) -> Result<Option<Site>> {
    let (pos, named) = args_of(f).ok_or_else(|| err(format!("{f}: bm25 takes a field, a text and options")))?;
    let [field, text] = pos.as_slice() else { return Err(err(format!("{f}: bm25 takes a TEXT field and the text to rank by"))) };
    let field = match field {
        Expr::Identifier(i) => i.value.clone(),
        Expr::CompoundIdentifier(p) => p.last().map(|i| i.value.clone()).unwrap_or_default(),
        e => return Err(err(format!("{f}: understand reads a field of the table, not {e}"))),
    };
    let mut opts = Options::default();
    for (name, _, e) in &named {
        match name.as_str() {
            UNDERSTAND => match compound::value_of(e, params)? {
                Value::Str(s) => opts.mode = Mode::of(&s)?,
                Value::Null => opts.mode = Mode::Off,
                v => return Err(err(format!("understand is one of 'off', 'boost', 'filter', not {}", crate::query::hint::sql_value(&v)))),
            },
            UNDERSTAND_FIELDS => match compound::value_of(e, params)? {
                Value::Str(s) => opts.fields = Some(s.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect()),
                Value::Null => {}
                v => return Err(err(format!("understand_fields is a list of fields, as text ('color, style'), not {}", crate::query::hint::sql_value(&v)))),
            },
            UNDERSTAND_WEIGHT => {
                opts.weight = match compound::value_of(e, params)?.as_f64() {
                    Some(w) if (0.0..=100.0).contains(&w) && w.is_finite() => w,
                    _ => return Err(err(format!("{f}: understand_weight is a number from 0 to 100"))),
                }
            }
            UNDERSTAND_SHARE => {
                opts.share = match compound::value_of(e, params)?.as_f64() {
                    Some(w) if (0.0..=1.0).contains(&w) => w,
                    _ => return Err(err(format!("{f}: understand_share is a share of rows, from 0 to 1"))),
                }
            }
            "syntax" => {
                if let Value::Str(s) = compound::value_of(e, params)? {
                    if s.eq_ignore_ascii_case("terms") {
                        return Err(err(format!("{f}: understand reads words as typed, not syntax => 'terms'")));
                    }
                }
            }
            _ => {}
        }
    }
    if opts.mode == Mode::Off {
        return Ok(None);
    }
    let Some(table) = ctx else {
        return Err(err(format!("{f}: understand reads the fields of one table: a SELECT ... FROM table, without a join")));
    };
    Ok(Some(Site { table: table.clone(), field, text: text.clone(), opts }))
}

fn context(q: &sql::Query) -> Option<String> {
    let sql::SetExpr::Select(s) = q.body.as_ref() else { return None };
    let [from] = s.from.as_slice() else { return None };
    if !from.joins.is_empty() {
        return None;
    }
    let sql::TableFactor::Table { name, args: None, .. } = &from.relation else { return None };
    Some(name.0.last()?.as_ident()?.value.clone())
}

struct Collect<'p> {
    ctx: Vec<Option<String>>,
    sites: Vec<Option<Site>>,
    params: &'p Params,
}

impl VisitorMut for Collect<'_> {
    type Break = Error;

    fn pre_visit_query(&mut self, q: &mut sql::Query) -> ControlFlow<Error> {
        self.ctx.push(context(q));
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _: &mut sql::Query) -> ControlFlow<Error> {
        self.ctx.pop();
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<Error> {
        if let Expr::Function(f) = e {
            if understands(f) {
                match site(f, self.ctx.last().and_then(|c| c.as_ref()), self.params) {
                    Ok(s) => self.sites.push(s),
                    Err(e) => return ControlFlow::Break(e),
                }
            }
        }
        ControlFlow::Continue(())
    }
}

struct Rewrite {
    with: Vec<Option<Understood>>,
    at: usize,
    /// The `WHERE` conditions of each query being visited.
    wheres: Vec<Vec<String>>,
}

fn parse_expr(text: &str) -> Result<Expr> {
    let dialect = sqlparser::dialect::GenericDialect {};
    sqlparser::parser::Parser::new(&dialect).try_with_sql(text).and_then(|mut p| p.parse_expr()).map_err(|e| err(format!("understand made {text}, which does not parse: {e}")))
}

impl VisitorMut for Rewrite {
    type Break = Error;

    fn pre_visit_query(&mut self, _: &mut sql::Query) -> ControlFlow<Error> {
        self.wheres.push(Vec::new());
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, q: &mut sql::Query) -> ControlFlow<Error> {
        let conds = self.wheres.pop().unwrap_or_default();
        if conds.is_empty() {
            return ControlFlow::Continue(());
        }
        let sql::SetExpr::Select(s) = q.body.as_mut() else { return ControlFlow::Continue(()) };
        let mut text: Vec<String> = s.selection.as_ref().map(|w| vec![format!("({w})")]).unwrap_or_default();
        text.extend(conds);
        match parse_expr(&text.join(" AND ")) {
            Ok(e) => s.selection = Some(e),
            Err(e) => return ControlFlow::Break(e),
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<Error> {
        let Expr::Function(f) = e else { return ControlFlow::Continue(()) };
        if !understands(f) {
            return ControlFlow::Continue(());
        }
        let i = self.at;
        self.at += 1;
        let Some((pos, named)) = args_of(f) else { return ControlFlow::Continue(()) };
        let mut args: Vec<FunctionArg> = pos.iter().map(|p| FunctionArg::Unnamed(FunctionArgExpr::Expr(p.clone()))).collect();
        args.extend(named.into_iter().filter(|(n, _, _)| !OWN.contains(&n.as_str())).map(|(_, a, _)| a));
        let u = self.with.get(i).cloned().flatten();
        if let Some(u) = &u {
            if u.ranked != u.text {
                args[1] = FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::value(sql::Value::SingleQuotedString(u.ranked.clone()))));
            }
        }
        if let FunctionArguments::List(l) = &mut f.args {
            l.args = args;
        }
        let Some(u) = u else { return ControlFlow::Continue(()) };
        if let Some(w) = self.wheres.last_mut() {
            w.extend(u.filters());
        }
        let boosts = u.boosts();
        if !boosts.is_empty() && u.weight > 0.0 {
            let cases: Vec<String> = boosts.iter().map(|c| format!("CASE WHEN {c} THEN 1 ELSE 0 END")).collect();
            let text = format!("{e} * (1 + {} * ({}))", num(u.weight), cases.join(" + "));
            match parse_expr(&text) {
                Ok(x) => *e = Expr::Nested(Box::new(x)),
                Err(x) => return ControlFlow::Break(x),
            }
        }
        ControlFlow::Continue(())
    }
}

/// The understanding of `q`, or `None` when no call understands.
pub fn plan(q: &sql::Query, params: &Params) -> Result<Option<Understanding>> {
    if !may_understand(&q.to_string()) {
        return Ok(None);
    }
    let mut query = q.clone();
    let mut c = Collect { ctx: Vec::new(), sites: Vec::new(), params };
    if let ControlFlow::Break(e) = query.visit(&mut c) {
        return Err(e);
    }
    if c.sites.is_empty() {
        return Ok(None);
    }
    Ok(Some(Understanding { query: q.clone(), sites: c.sites }))
}

/// [`plan`] of the query of text `sql`.
pub fn plan_sql(sql: &str, params: &Params) -> Result<Option<Understanding>> {
    if !may_understand(sql) {
        return Ok(None);
    }
    let Ok(p) = super::sql::parse_query_cached(sql) else { return Ok(None) };
    plan(&p.query, params)
}

/// What a caller looks up for [`Understanding::finish`]: a gazetteer of
/// each (table, keyword field, analyzer), built from the answer of
/// [`dictionary_sql`].
pub type Gazetteers = HashMap<(String, String, Analyzer), Arc<Gazetteer>>;

fn schema_of<'a>(catalog: &'a [Arc<Schema>], table: &str) -> Result<&'a Arc<Schema>> {
    catalog.iter().find(|s| s.table.eq_ignore_ascii_case(table)).ok_or_else(|| err(format!("no table {table}")))
}

fn analyzer_of(schema: &Schema, field: &str) -> Result<Analyzer> {
    Ok(schema.field(field).ok_or_else(|| err(format!("{} has no field {field}", schema.table)))?.text_analyzer())
}

/// The gazetteers understanding `field` of `schema` with `opts` needs:
/// (table, keyword field, the text field's analyzer).
pub fn needs_of(schema: &Schema, field: &str, opts: &Options) -> Result<Vec<(String, String, Analyzer)>> {
    let a = analyzer_of(schema, field)?;
    Ok(fields(schema, field, opts.fields.as_deref())?.keywords.into_iter().map(|k| (schema.table.clone(), k, a)).collect())
}

/// What `text` says for `field` of `schema` with `opts`, from the
/// gazetteers [`needs_of`] names.
pub fn understand_in(schema: &Schema, field: &str, text: &str, opts: &Options, gaz: &Gazetteers) -> Result<Understood> {
    let a = analyzer_of(schema, field)?;
    let fs = fields(schema, field, opts.fields.as_deref())?;
    let mut gs: Vec<&Gazetteer> = Vec::new();
    let mut skipped = Vec::new();
    for k in &fs.keywords {
        match gaz.get(&(schema.table.clone(), k.clone(), a)) {
            Some(g) if g.cut && !fs.listed => skipped.push(k.clone()),
            Some(g) => gs.push(g),
            None => {}
        }
    }
    let mut u = understand(&schema.table, field, text, opts, &gs, &fs.numbers, a);
    u.skipped = skipped;
    Ok(u)
}

impl Understanding {
    /// The gazetteers [`Understanding::finish`] needs.
    pub fn needs(&self, catalog: &[Arc<Schema>]) -> Result<Vec<(String, String, Analyzer)>> {
        let mut out = Vec::new();
        for s in self.sites.iter().flatten() {
            for n in needs_of(schema_of(catalog, &s.table)?, &s.field, &s.opts)? {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        Ok(out)
    }

    /// What each call understood, before [`Understood::confirm`].
    pub fn understood(&self, catalog: &[Arc<Schema>], gaz: &Gazetteers, params: &Params) -> Result<Vec<Understood>> {
        let mut made = Vec::new();
        for s in self.sites.iter().flatten() {
            let text = match compound::value_of(&s.text, params)? {
                Value::Str(t) => t,
                Value::Null => String::new(),
                v => return Err(err(format!("bm25's text is a string, not {}", crate::query::hint::sql_value(&v)))),
            };
            made.push(understand_in(schema_of(catalog, &s.table)?, &s.field, &text, &s.opts, gaz)?);
        }
        Ok(made)
    }

    /// The query with each call rewritten by what it understood (`made`,
    /// [`Understanding::understood`]'s, confirmed).
    pub fn rewrite(&self, made: &[Understood]) -> Result<sql::Query> {
        let mut made = made.iter();
        let with = self.sites.iter().map(|s| s.as_ref().and_then(|_| made.next().cloned())).collect();
        let mut q = self.query.clone();
        let mut r = Rewrite { with, at: 0, wheres: Vec::new() };
        if let ControlFlow::Break(e) = q.visit(&mut r) {
            return Err(e);
        }
        Ok(q)
    }

    /// [`Understanding::understood`] and [`Understanding::rewrite`], each
    /// run's share counted by `count` (a query's single number).
    pub fn finish(&self, catalog: &[Arc<Schema>], gaz: &Gazetteers, params: &Params, mut count: impl FnMut(&str) -> Result<u64>) -> Result<(sql::Query, Vec<Understood>)> {
        let mut made = self.understood(catalog, gaz, params)?;
        for u in &mut made {
            let counts = u.checks().iter().map(|c| count(c)).collect::<Result<Vec<_>>>()?;
            u.confirm(&counts);
        }
        Ok((self.rewrite(&made)?, made))
    }
}

/// The number a `SELECT COUNT(*)` answered.
pub fn count_of(rs: &ResultSet) -> u64 {
    rs.rows.first().and_then(|r| r.first()).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gaz() -> Vec<Gazetteer> {
        let a = Analyzer::English;
        vec![
            Gazetteer::of("class", &[("Coffee & Cocktail Tables", 100), ("Coffee Tables", 10), ("Accent Chairs", 50), ("Dining Chairs", 40), ("Table Lamps", 30)], a),
            Gazetteer::of("color", &[("white", 200), ("off white", 5), ("black", 100), ("2", 9)], a),
            Gazetteer::of("finish", &[("white", 20)], a),
        ]
    }

    fn opts(mode: Mode) -> Options {
        Options { mode, ..Options::default() }
    }

    #[test]
    fn words_name_values_by_their_heads() {
        let g = gaz();
        let gs: Vec<&Gazetteer> = g.iter().collect();
        let u = understand("p", "body", "white coffee table", &opts(Mode::Filter), &gs, &[], Analyzer::English);
        assert_eq!(u.spans.len(), 2, "{u:?}");
        assert_eq!(u.spans[0].words, "white");
        // A value that holds the words, its head last, is named too.
        assert_eq!(u.spans[0].condition(), "(color IN ('white', 'off white') OR finish = 'white')");
        assert_eq!(u.spans[1].words, "coffee table");
        assert_eq!(u.spans[1].condition(), "class IN ('Coffee Tables', 'Coffee & Cocktail Tables')");
        assert!(u.spans[1].filters);
        // `table` alone is the head of no value but `Coffee ... Tables`:
        // not `Table Lamps`, whose head is lamps.
        let u = understand("p", "body", "chair", &opts(Mode::Filter), &gs, &[], Analyzer::English);
        assert_eq!(u.spans[0].condition(), "class IN ('Accent Chairs', 'Dining Chairs')");
        assert!(!u.spans[0].filters, "a partial match boosts");
        assert_eq!(u.filters(), Vec::<String>::new());
        assert_eq!(u.boosts().len(), 1);
    }

    #[test]
    fn numbers_say_ranges() {
        let a = Analyzer::English;
        let r = ranges("sofa under $1,500", &["price".into()], a);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].condition(), "price < 1500");
        assert_eq!(r[0].words, "under $1,500");
        let r = ranges("rug between 100 and 200 dollars", &["price".into()], a);
        assert_eq!(r[0].condition(), "price >= 100 AND price <= 200");
        assert_eq!(r[0].words, "between 100 and 200 dollars");
        let r = ranges("papers since 2020 with over 10 citations", &["year".into(), "citations".into()], a);
        assert_eq!(r.iter().map(|r| r.condition()).collect::<Vec<_>>(), vec!["year >= 2020", "citations > 10"]);
        let u = understand("p", "body", "sofa under $1,500", &opts(Mode::Boost), &[], &["price".into()], a);
        assert_eq!(u.ranked, "sofa");
        assert!(ranges("2 seater sofa", &["price".into()], a).is_empty());
    }

    #[test]
    fn plan_rewrites_calls() {
        let q = super::super::sql::parse_query("SELECT id FROM products WHERE rating > 3 ORDER BY bm25(body, :q, understand => 'filter') DESC LIMIT 10").unwrap();
        let p = Params::new().set("q", Value::Str("white coffee table under 300".into()));
        let u = plan(&q, &p).unwrap().unwrap();
        let mut schema = Schema::new(
            "products",
            vec![
                crate::schema::Field::new("id", FieldType::Keyword),
                crate::schema::Field::new("body", FieldType::Text { positions: false }),
                crate::schema::Field::new("class", FieldType::Keyword),
                crate::schema::Field::new("color", FieldType::Keyword),
                crate::schema::Field::new("finish", FieldType::Keyword),
                crate::schema::Field::new("price", FieldType::Float),
            ],
            Some("id"),
        )
        .unwrap();
        schema.table = "products".into();
        let catalog = vec![Arc::new(schema)];
        let needs = u.needs(&catalog).unwrap();
        assert_eq!(needs.iter().map(|n| n.1.as_str()).collect::<Vec<_>>(), vec!["class", "color", "finish"]);
        let g = gaz();
        let gaz: Gazetteers = g.into_iter().map(|g| (("products".to_string(), g.field.clone(), Analyzer::English), Arc::new(g))).collect();
        let (q2, made) = u.finish(&catalog, &gaz, &p, |_| Ok(0)).unwrap();
        let text = q2.to_string();
        // A filter keeps the values named in full, and rows without one.
        assert!(text.contains("WHERE (rating > 3) AND (color = 'white' OR finish = 'white' OR color IS NULL AND finish IS NULL) AND (class = 'Coffee Tables' OR class IS NULL)"), "{text}");
        assert!(!text.contains("price"), "{text}: price is not listed");
        // and boosts by every value.
        assert!(text.contains("CASE WHEN class IN ('Coffee Tables', 'Coffee & Cocktail Tables') THEN 1 ELSE 0 END"), "{text}");
        assert!(!text.contains("understand"), "{text}");
        assert_eq!(made.len(), 1);
        // A run most rows saying its words do not hold is left out.
        let (q2, made) = u.finish(&catalog, &gaz, &p, |c| Ok(if c.contains("class") && c.contains(" AND ") { 1 } else { 100 })).unwrap();
        assert!(!q2.to_string().contains("class"), "{q2}");
        assert_eq!(made[0].dropped.len(), 1);
        assert_eq!(made[0].dropped[0].share, Some(0.01));
        // A boost ranks by the conditions a row meets.
        let q = super::super::sql::parse_query("SELECT id FROM products ORDER BY bm25(body, :q, understand => 'boost', understand_fields => 'class, price') DESC LIMIT 10").unwrap();
        let (q2, made) = plan(&q, &p).unwrap().unwrap().finish(&catalog, &gaz, &p, |_| Ok(0)).unwrap();
        assert_eq!(q2.to_string(), "SELECT id FROM products ORDER BY (bm25(body, 'white coffee table') * (1 + 0.25 * (CASE WHEN class IN ('Coffee Tables', 'Coffee & Cocktail Tables') THEN 1 ELSE 0 END + CASE WHEN price < 300 THEN 1 ELSE 0 END))) DESC LIMIT 10");
        assert_eq!(made[0].line(), "bm25(body) understood 'white coffee table under 300' with boost of products: boost 0.25 for each of class IN ('Coffee Tables', 'Coffee & Cocktail Tables'), price < 300, ranking by 'white coffee table'");
        // understand => 'off' is the call as written.
        let q = super::super::sql::parse_query("SELECT id FROM products ORDER BY bm25(body, :q, understand => 'off', understand_weight => 2) DESC").unwrap();
        let u = plan(&q, &p).unwrap().unwrap();
        let (q2, made) = u.finish(&catalog, &gaz, &p, |_| Ok(0)).unwrap();
        assert_eq!(q2.to_string(), "SELECT id FROM products ORDER BY bm25(body, :q) DESC");
        assert!(made.is_empty());
    }
}
