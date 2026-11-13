//! Query expansion by pseudo-relevance feedback (RM3):
//! `bm25(body, :q, expand => 10)` ranks by the query's words and the words
//! its first ranking's top rows hold most.
//!
//! ```sql
//! SELECT id, title FROM papers ORDER BY bm25(body, :q, expand => 10) DESC LIMIT 10;
//! ```
//!
//! A query that expands runs in two steps. The first ranks the rows of
//! each expanded call as written, without `expand`: `SELECT body, score()
//! FROM papers WHERE <the query's WHERE> ORDER BY bm25(body, :q) DESC LIMIT
//! 10`. Each of those rows weighs its terms by its share of the scores and
//! each term's share of the row; the [`DEFAULT_TERMS`] best terms over all
//! of them, scaled to sum to `1 − weight`, join the query's own terms,
//! scaled to sum to `weight` ([`DEFAULT_WEIGHT`]). The second step is the
//! query with each call rewritten to those terms, analysed and weighted
//! (`bm25(body, 'vaccin^0.2833 mrna^0.2833 trial^0.0412 ...', syntax =>
//! 'terms')`), which plans and runs as any query does. The rewrite is
//! shown in the answer's plan lines.
//!
//! Options of the call: `expand => n` the rows read (0 leaves the call as
//! it is; at most [`MAX_ROWS`]), `expand_terms => k` the terms added (at
//! most [`MAX_TERMS`]), `expand_weight => w` the query's own share, from 0
//! to 1. Each may be a parameter (`expand => :expand`).
//!
//! On a coordinator over shards the first step is an ordinary query of the
//! shards, merged, and the rewrite is made on the coordinator: every shard
//! ranks by the same terms, as one database would.
//!
//! The settings were chosen on SciFact's train and NFCorpus's dev
//! judgements; on BEIR's test judgements they move nDCG@10 from 0.691 to
//! 0.693 on SciFact, 0.329 to 0.346 on NFCorpus and 0.362 to 0.369 on
//! ArguAna (`chloedb-bm25-eval`, variant `rm3`).

use std::ops::ControlFlow;

use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, VisitMut, VisitorMut};

use crate::error::{Error, Result};
use crate::query::exec::ResultSet;
use crate::query::{compound, Params};
use crate::schema::Schema;
use crate::text::Analyzer;
use crate::value::Value;

/// The option that turns expansion on: the rows read.
pub const EXPAND: &str = "expand";
/// The option of the terms added.
pub const EXPAND_TERMS: &str = "expand_terms";
/// The option of the query's own share of the weights.
pub const EXPAND_WEIGHT: &str = "expand_weight";
/// Terms added when `expand_terms` is not given.
pub const DEFAULT_TERMS: usize = 20;
/// The query's own share when `expand_weight` is not given.
pub const DEFAULT_WEIGHT: f64 = 0.85;
/// The most rows `expand` reads.
pub const MAX_ROWS: usize = 100;
/// The most terms `expand_terms` adds.
pub const MAX_TERMS: usize = 100;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// Whether the text of a query may expand: a test that costs nothing,
/// before [`plan`] looks.
pub fn may_expand(sql: &str) -> bool {
    let b = sql.as_bytes();
    b.windows(EXPAND.len()).any(|w| w.eq_ignore_ascii_case(EXPAND.as_bytes()))
}

/// One expanded `bm25` call.
#[derive(Clone, Debug)]
struct Site {
    table: String,
    field: String,
    /// The text, a literal or a parameter.
    text: Expr,
    /// The call's other options, as written.
    rest: Vec<FunctionArg>,
    /// Whether the query's stopwords are dropped (`stopwords =>`), else the
    /// analyzer's choice.
    stopwords: Option<bool>,
    rows: usize,
    terms: usize,
    weight: f64,
    /// The `WHERE` of the query the call ranks.
    filter: Option<Expr>,
}

/// A query that expands: the queries of its first step, then
/// [`Expansion::finish`].
#[derive(Clone, Debug)]
pub struct Expansion {
    query: sql::Query,
    /// Each call that names `expand`, in the order of the query's text;
    /// `None` for one with `expand => 0`.
    sites: Vec<Option<Site>>,
}

/// What a call's expansion added, for the answer's plan lines and for
/// previews.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Expanded {
    pub table: String,
    pub field: String,
    /// The rows read.
    pub rows: usize,
    /// The terms ranked by, with their weights, the query's own first.
    pub terms: Vec<(String, f64)>,
    /// How many of [`Expanded::terms`] the query had.
    pub own: usize,
}

impl Expanded {
    /// The plan line of the expansion.
    pub fn line(&self) -> String {
        let added: Vec<String> = self.terms[self.own..].iter().map(|(t, w)| format!("{t} {w:.3}")).collect();
        format!("bm25({}) expanded from {} rows of {} with {} terms: {}", self.field, self.rows, self.table, added.len(), added.join(", "))
    }
}

fn name_is(f: &sql::Function, name: &str) -> bool {
    f.name.to_string().eq_ignore_ascii_case(name)
}

/// The arguments of a call, positional and named (lowercased).
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

/// Whether `f` is a `bm25` call that expands.
fn expands(f: &sql::Function) -> bool {
    name_is(f, "bm25") && args_of(f).is_some_and(|(_, named)| named.iter().any(|(n, _, _)| n == EXPAND))
}

fn whole(v: &Value, what: &str, max: usize) -> Result<usize> {
    match v {
        Value::Int(n) if *n >= 0 && (*n as usize) <= max => Ok(*n as usize),
        Value::Float(x) if x.fract() == 0.0 && *x >= 0.0 && *x <= max as f64 => Ok(*x as usize),
        v => Err(err(format!("bm25's {what} is a whole number from 0 to {max}, not {}", crate::query::hint::sql_value(v)))),
    }
}

/// The site of an expanded call, in a query of `table` filtered by
/// `filter`; `None` when `expand => 0`.
fn site(f: &sql::Function, ctx: Option<&(String, Option<Expr>)>, params: &Params) -> Result<Option<Site>> {
    let (pos, named) = args_of(f).ok_or_else(|| err(format!("{f}: bm25 takes a field, a text and options")))?;
    let [field, text] = pos.as_slice() else { return Err(err(format!("{f}: bm25 takes a TEXT field and the text to rank by"))) };
    let field = match field {
        Expr::Identifier(i) => i.value.clone(),
        Expr::CompoundIdentifier(p) => p.last().map(|i| i.value.clone()).unwrap_or_default(),
        e => return Err(err(format!("{f}: expand ranks a field of the table, not {e}"))),
    };
    let (mut rows, mut terms, mut weight, mut stopwords) = (0, DEFAULT_TERMS, DEFAULT_WEIGHT, None);
    let mut rest = Vec::new();
    for (name, arg, e) in &named {
        match name.as_str() {
            EXPAND => rows = whole(&compound::value_of(e, params)?, "expand", MAX_ROWS)?,
            EXPAND_TERMS => terms = whole(&compound::value_of(e, params)?, "expand_terms", MAX_TERMS)?,
            EXPAND_WEIGHT => {
                weight = match compound::value_of(e, params)?.as_f64() {
                    Some(w) if (0.0..=1.0).contains(&w) => w,
                    _ => return Err(err(format!("{f}: expand_weight is the query's own share of the weights, from 0 to 1"))),
                }
            }
            "min" | "min_rows" | "max_rows" => return Err(err(format!("{f}: a call that expands ranks every row with one of its terms: it takes no {name}"))),
            "stopwords" => {
                stopwords = Some(compound::value_of(e, params)?.as_f64().ok_or_else(|| err("stopwords => 0 or 1"))? != 0.0);
                rest.push(arg.clone());
            }
            _ => rest.push(arg.clone()),
        }
    }
    if rows == 0 {
        return Ok(None);
    }
    let Some((table, filter)) = ctx else {
        return Err(err(format!("{f}: expand ranks the rows of one table: a SELECT ... FROM table, without a join")));
    };
    Ok(Some(Site { table: table.clone(), field, text: text.clone(), rest, stopwords, rows, terms, weight, filter: filter.clone() }))
}

/// The table a query reads and its `WHERE`, when it reads one table.
fn context(q: &sql::Query) -> Option<(String, Option<Expr>)> {
    let sql::SetExpr::Select(s) = q.body.as_ref() else { return None };
    let [from] = s.from.as_slice() else { return None };
    if !from.joins.is_empty() {
        return None;
    }
    let sql::TableFactor::Table { name, args: None, .. } = &from.relation else { return None };
    let table = name.0.last()?.as_ident()?.value.clone();
    Some((table, s.selection.clone()))
}

/// Walks a query's expanded calls in order: [`Collect`] finds them,
/// [`Rewrite`] replaces them.
struct Collect<'p> {
    ctx: Vec<Option<(String, Option<Expr>)>>,
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
            if expands(f) {
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
    /// Each call's terms, in [`Collect`]'s order; `None` for a call left
    /// as written.
    with: Vec<Option<(Site, Vec<(String, f64)>)>>,
    at: usize,
}

impl VisitorMut for Rewrite {
    type Break = Error;

    fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<Error> {
        let Expr::Function(f) = e else { return ControlFlow::Continue(()) };
        if !expands(f) {
            return ControlFlow::Continue(());
        }
        let i = self.at;
        self.at += 1;
        let Some((pos, named)) = args_of(f) else { return ControlFlow::Continue(()) };
        let mut args: Vec<FunctionArg> = pos.iter().map(|p| FunctionArg::Unnamed(FunctionArgExpr::Expr(p.clone()))).collect();
        match self.with.get(i).cloned().flatten() {
            // `expand => 0`, or no rows to read: the call as written,
            // without the expansion's options.
            None => args.extend(named.into_iter().filter(|(n, _, _)| ![EXPAND, EXPAND_TERMS, EXPAND_WEIGHT].contains(&n.as_str())).map(|(_, a, _)| a)),
            Some((site, terms)) => {
                let text: Vec<String> = terms.iter().map(|(t, w)| format!("{t}^{w:.6}")).collect();
                args[1] = FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::value(sql::Value::SingleQuotedString(text.join(" ")))));
                args.extend(site.rest.into_iter().filter(|a| !matches!(a, FunctionArg::Named { name, .. } if name.value.eq_ignore_ascii_case("syntax") || name.value.eq_ignore_ascii_case("stopwords"))));
                args.push(named_arg("syntax", Expr::value(sql::Value::SingleQuotedString("terms".into()))));
            }
        }
        if let FunctionArguments::List(l) = &mut f.args {
            l.args = args;
        }
        ControlFlow::Continue(())
    }
}

fn named_arg(name: &str, e: Expr) -> FunctionArg {
    FunctionArg::Named { name: Ident::new(name), arg: FunctionArgExpr::Expr(e), operator: sql::FunctionArgOperator::RightArrow }
}

/// The expansion of `q`, or `None` when it expands no call.
pub fn plan(q: &sql::Query, params: &Params) -> Result<Option<Expansion>> {
    if !may_expand(&q.to_string()) {
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
    Ok(Some(Expansion { query: q.clone(), sites: c.sites }))
}

/// [`plan`] of the query of text `sql`: `None` when it expands no call
/// (or does not parse, which fails where it would without one).
pub fn plan_sql(sql: &str, params: &Params) -> Result<Option<Expansion>> {
    if !may_expand(sql) {
        return Ok(None);
    }
    let Ok(p) = super::sql::parse_query_cached(sql) else { return Ok(None) };
    plan(&p.query, params)
}

/// The columns that hold the text of `field`: the field, or for one that
/// is not stored (a generated TEXT field) the stored fields it is made of.
pub fn text_columns(schemas: &[&Schema], table: &str, field: &str) -> Vec<String> {
    let Some(schema) = schemas.iter().find(|s| s.table.eq_ignore_ascii_case(table)) else { return vec![field.to_string()] };
    match schema.field(field) {
        Some(f) if !f.stored && !f.generated_from.is_empty() => {
            let from: Vec<String> = f.generated_from.iter().filter(|g| schema.field(g).is_some_and(|g| g.stored)).cloned().collect();
            if from.is_empty() {
                vec![field.to_string()]
            } else {
                from
            }
        }
        _ => vec![field.to_string()],
    }
}

impl Expansion {
    /// The queries of the first step, one for each call that expands: its
    /// rows' text (the columns [`text_columns`] names, of the tables of
    /// `schemas`) and score, best first.
    pub fn feedback(&self, schemas: &[&Schema]) -> Vec<sql::Query> {
        self.sites.iter().flatten().map(|s| {
            let mut args = vec![
                FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Identifier(Ident::new(&s.field)))),
                FunctionArg::Unnamed(FunctionArgExpr::Expr(s.text.clone())),
            ];
            args.extend(s.rest.iter().cloned());
            let call = Expr::Function(sql::Function {
                name: sql::ObjectName::from(vec![Ident::new("bm25")]),
                uses_odbc_syntax: false,
                parameters: FunctionArguments::None,
                args: FunctionArguments::List(sql::FunctionArgumentList { duplicate_treatment: None, args, clauses: Vec::new() }),
                filter: None,
                null_treatment: None,
                over: None,
                within_group: Vec::new(),
            });
            let filter = s.filter.as_ref().map(|f| format!(" WHERE {f}")).unwrap_or_default();
            let cols: Vec<String> = text_columns(schemas, &s.table, &s.field).iter().map(|c| Ident::new(c).to_string()).collect();
            let text = format!("SELECT {}, score() FROM {}{filter} ORDER BY {call} DESC LIMIT {}", cols.join(", "), Ident::new(&s.table), s.rows);
            let q = super::sql::parse_query(&text).expect("a feedback query parses");
            (*q).clone()
        }).collect()
    }

    /// The tables and fields of [`Expansion::feedback`]'s queries, whose
    /// analyzers [`Expansion::finish`] takes in the same order.
    pub fn fields(&self) -> Vec<(String, String)> {
        self.sites.iter().flatten().map(|s| (s.table.clone(), s.field.clone())).collect()
    }

    /// The query of the second step: each call ranked by its query's terms
    /// and those its rows (`rows`, the answers of
    /// [`Expansion::feedback`]'s queries) hold most, analysed by
    /// `analyzers` ([`Expansion::fields`]'s). Also what each call added.
    pub fn finish(&self, rows: &[ResultSet], analyzers: &[Analyzer], params: &Params) -> Result<(sql::Query, Vec<Expanded>)> {
        let mut with = Vec::new();
        let mut made = Vec::new();
        let mut i = 0;
        for s in &self.sites {
            let Some(s) = s else {
                with.push(None);
                continue;
            };
            let (rs, a) = (&rows[i], analyzers[i]);
            i += 1;
            let text = match compound::value_of(&s.text, params)? {
                Value::Str(t) => t,
                Value::Null => String::new(),
                v => return Err(err(format!("bm25's text is a string, not {}", crate::query::hint::sql_value(&v)))),
            };
            let drop = s.stopwords.unwrap_or(a.drops_query_stopwords());
            let own = a.query_term_seq(&text, drop);
            // The text columns, then the score.
            let docs: Vec<(String, f64)> = rs
                .rows
                .iter()
                .filter_map(|r| {
                    let (score, text) = r.split_last()?;
                    let text: Vec<&str> = text.iter().filter_map(|v| v.as_str()).collect();
                    if text.is_empty() {
                        return None;
                    }
                    Some((text.join(" . "), score.as_f64()?))
                })
                .collect();
            match rm3(&own, &docs, a, s.terms, s.weight) {
                Some((terms, n_own)) => {
                    made.push(Expanded { table: s.table.clone(), field: s.field.clone(), rows: docs.len(), terms: terms.clone(), own: n_own });
                    with.push(Some((s.clone(), terms)));
                }
                None => with.push(None),
            }
        }
        let mut q = self.query.clone();
        let mut r = Rewrite { with, at: 0 };
        if let ControlFlow::Break(e) = q.visit(&mut r) {
            return Err(e);
        }
        Ok((q, made))
    }
}

/// RM3's terms: `own` (the query's analysed terms) sharing `weight`, and
/// the `k` terms the rows `docs` (text and score) hold most sharing the
/// rest, each row's terms weighed by its share of the scores and each
/// term's share of the row. The query's terms come first; with their
/// count. `None` when there is nothing to rank by, or no row.
pub fn rm3(own: &[String], docs: &[(String, f64)], a: Analyzer, k: usize, weight: f64) -> Option<(Vec<(String, f64)>, usize)> {
    if own.is_empty() || docs.is_empty() {
        return None;
    }
    let total: f64 = docs.iter().map(|d| d.1.max(0.0)).sum();
    let mut rel: std::collections::HashMap<String, f64> = Default::default();
    for (text, score) in docs {
        let share = if total > 0.0 { score.max(0.0) / total } else { 1.0 / docs.len() as f64 };
        let ts = a.analyze(text);
        let len = ts.len().max(1) as f64;
        for t in ts {
            // Numbers say little of what a text is about.
            if t.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            *rel.entry(t).or_default() += share / len;
        }
    }
    let mut rel: Vec<(String, f64)> = rel.into_iter().collect();
    rel.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rel.truncate(k);
    let sum: f64 = rel.iter().map(|x| x.1).sum();
    let mut out: Vec<(String, f64)> = Vec::new();
    for t in own {
        let w = weight / own.len() as f64;
        match out.iter_mut().find(|x| &x.0 == t) {
            Some(x) => x.1 += w,
            None => out.push((t.clone(), w)),
        }
    }
    let n_own = out.len();
    if sum > 0.0 {
        for (t, x) in rel {
            let w = (1.0 - weight) * x / sum;
            match out.iter_mut().find(|y| y.0 == t) {
                Some(y) => y.1 += w,
                None => out.push((t, w)),
            }
        }
    }
    out.retain(|(_, w)| *w > 0.0);
    (!out.is_empty()).then_some((out, n_own))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(xs: &[(&str, f64)]) -> Vec<(String, f64)> {
        xs.iter().map(|(t, s)| (t.to_string(), *s)).collect()
    }

    #[test]
    fn rm3_mixes_the_query_with_its_rows_terms() {
        let own = Analyzer::English.query_term_seq("mrna vaccines", true);
        let docs = rows(&[("mRNA vaccines train the immune system", 3.0), ("vaccines and the immune response", 1.0)]);
        let (terms, n) = rm3(&own, &docs, Analyzer::English, 3, 0.5).unwrap();
        assert_eq!(n, 2);
        assert_eq!(terms[0].0, "mrna");
        // The query's own terms share half, the added ones the other half.
        let own_sum: f64 = terms[..n].iter().map(|t| t.1).sum();
        assert!(own_sum > 0.5 - 1e-9, "{terms:?}");
        let total: f64 = terms.iter().map(|t| t.1).sum();
        assert!((total - 1.0).abs() < 1e-9, "{terms:?}");
        assert!(terms.iter().any(|t| t.0 == "immun"), "{terms:?}");
    }

    #[test]
    fn rm3_without_rows_or_words_adds_nothing() {
        assert!(rm3(&[], &rows(&[("text", 1.0)]), Analyzer::English, 5, 0.5).is_none());
        assert!(rm3(&["x".into()], &[], Analyzer::English, 5, 0.5).is_none());
    }

    #[test]
    fn plan_finds_calls_and_rewrites_them() {
        let q = super::super::sql::parse_query("SELECT id FROM papers WHERE year > 2020 ORDER BY bm25(body, :q, expand => 5, expand_terms => 2) DESC LIMIT 3").unwrap();
        let p = Params::new().set("q", Value::Str("mrna vaccines".into()));
        let x = plan(&q, &p).unwrap().unwrap();
        let fb = x.feedback(&[]);
        assert_eq!(fb.len(), 1);
        assert_eq!(fb[0].to_string(), "SELECT body, score() FROM papers WHERE year > 2020 ORDER BY bm25(body, :q) DESC LIMIT 5");
        assert_eq!(x.fields(), vec![("papers".to_string(), "body".to_string())]);
        let rs = ResultSet { columns: vec!["body".into(), "score()".into()], rows: vec![vec![Value::Str("mRNA vaccines and immune cells".into()), Value::Float(2.0)]], keys: Vec::new(), scores: Vec::new(), parts: None, stats: Default::default() };
        let (q2, made) = x.finish(&[rs], &[Analyzer::English], &p).unwrap();
        let text = q2.to_string();
        assert!(text.contains("syntax => 'terms'"), "{text}");
        assert!(!text.contains("expand"), "{text}");
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].terms.len(), 4, "{made:?}");
    }

    #[test]
    fn expand_zero_is_the_call_as_written() {
        let q = super::super::sql::parse_query("SELECT id FROM papers ORDER BY bm25(body, :q, expand => 0, syntax => 'plain') DESC").unwrap();
        let x = plan(&q, &Params::new()).unwrap().unwrap();
        assert!(x.feedback(&[]).is_empty());
        let (q2, made) = x.finish(&[], &[], &Params::new()).unwrap();
        assert!(made.is_empty());
        assert_eq!(q2.to_string(), "SELECT id FROM papers ORDER BY bm25(body, :q, syntax => 'plain') DESC");
    }

    #[test]
    fn a_join_does_not_expand() {
        let q = super::super::sql::parse_query("SELECT a.id FROM papers a JOIN papers b ON a.id = b.id ORDER BY bm25(a.body, 'x', expand => 5) DESC").unwrap();
        assert!(plan(&q, &Params::new()).is_err());
    }
}
