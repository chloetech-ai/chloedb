//! `PACK n TOKENS`: a query's answer as the context a language model is
//! given, filled to a budget of tokens rather than cut to k rows.
//!
//! ```sql
//! SELECT id, kind, text FROM memory
//! WHERE superseded = false
//! ORDER BY rrf(bm25(text, :task), similarity(emb, :q)) DESC
//! PACK 4000 TOKENS DEDUPE 0.85 FOLLOW links 1 HOP WITH RECEIPT
//! ```
//!
//! The rows are read in the query's order, in growing slices (64, then four
//! times as many, up to the query's `LIMIT`, 1,000 without one), and each
//! candidate is decided once, in order:
//!
//! - **take** it whole when its text fits what is left of the budget;
//! - **excerpt** it when it is longer than a row may be (`EXCERPT n`, a
//!   quarter of the budget by default) or than what is left: the stretch of
//!   its longest text column that holds the most words of the query's
//!   `bm25` and `MATCH` texts, cut to fit;
//! - **skip: near-duplicate of k** when `DEDUPE x` is given and the row
//!   says what a row taken before says: most of the shorter one's words are
//!   in the other (the overlap of their word sets is at least x), or, with
//!   `DEDUPE x ON emb`, the cosine of their vectors is at least x;
//! - **skip: over budget** when not even an excerpt fits.
//!
//! With `FOLLOW edges n HOP`, each row taken brings the rows its `EDGES`
//! field reaches within n hops (one by default) right after it, if they pass
//! the query's `WHERE`, decided the same way (**linked from k**). Reading
//! stops once what is left of the budget is too small for any row, so a
//! query whose first rows fill the budget reads one slice.
//!
//! The answer is the rows taken, in that order, with two more columns:
//! `pack_tokens`, the tokens the row spends, and `pack_decision`. `WITH
//! RECEIPT` adds the rows skipped and the columns `pack_score` (the
//! query's score of the row) and `pack_used` (the tokens spent up to and
//! with the row). [`Packed::receipt`] says the same, with the snapshot the
//! rows were read from.
//!
//! Tokens are counted by [`count_tokens`], an estimate: a tokenizer's count
//! depends on the model, and the estimate is meant to be within a tenth of
//! the usual BPE tokenizers' on English prose and code.
//!
//! The clause is taken off the statement's text before it is parsed, and the
//! rest runs as any query does, once per slice (and once per row followed),
//! so PACK works wherever a query does: on a file, a server or a
//! coordinator over shards. Nothing about it is stored.

use std::collections::{HashSet, VecDeque};

use sqlparser::ast as sql;
use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Location, Token, Tokenizer};

use crate::error::{Error, Result};
use crate::query::exec::{ExecStats, ResultSet};
use crate::query::sql::Params;
use crate::value::Value;

/// The rows read without a `LIMIT`.
pub const DEFAULT_CANDIDATES: usize = 1000;
/// The rows of the first slice.
const FIRST_SLICE: usize = 64;
/// The rows a row followed brings at most.
const FOLLOWED: usize = 16;
/// The fewest tokens an excerpt is made of: below this, a row that does not
/// fit is skipped.
const MIN_EXCERPT: usize = 32;
/// The largest budget.
const MAX_BUDGET: usize = 10_000_000;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// An estimate of the tokens a language model's tokenizer makes of `text`.
///
/// A run of letters is one token per six letters or part of them (most
/// English words are one; `idempotency` is two), a run of digits one per
/// three, a letter of a script written without spaces (CJK) one each, and
/// any other character but whitespace one each. Whitespace joins the token
/// after it, as BPE tokenizers make " word" one token.
pub fn count_tokens(text: &str) -> usize {
    let mut n = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c.is_ascii_digit() {
            let mut len = 1usize;
            while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                chars.next();
                len += 1;
            }
            n += len.div_ceil(3);
        } else if c.is_alphabetic() && !spaceless(c) {
            let mut len = 1usize;
            while chars.peek().is_some_and(|d| d.is_alphabetic() && !spaceless(*d)) {
                chars.next();
                len += 1;
            }
            n += len.div_ceil(6);
        } else {
            n += 1;
        }
    }
    n
}

/// Whether `c` is of a script written without spaces between words, whose
/// letters BPE tokenizers make a token or more each.
fn spaceless(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0x0e00..=0x0e7f)
}

/// What a `PACK` clause asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// `PACK n TOKENS`.
    pub budget: usize,
    /// `OF a, b`: the columns whose text is counted (all text columns but
    /// the key's without it).
    pub of: Vec<String>,
    /// `EXCERPT n`: the most tokens one row may spend.
    pub excerpt: Option<usize>,
    /// `DEDUPE x [ON column]`.
    pub dedupe: Option<(f64, Option<String>)>,
    /// `FOLLOW field [n HOP]`.
    pub follow: Option<(String, usize)>,
    /// `WITH RECEIPT`.
    pub receipt: bool,
}

impl Spec {
    /// The most tokens one row may spend.
    pub fn row_cap(&self) -> usize {
        self.excerpt.unwrap_or((self.budget / 4).max(MIN_EXCERPT)).min(self.budget)
    }
}

/// Whether `sql_text` may hold a `PACK` clause: a cheap test before
/// [`split`] tokenizes it.
pub fn may_pack(sql_text: &str) -> bool {
    sql_text.as_bytes().windows(4).any(|w| w.eq_ignore_ascii_case(b"pack"))
}

/// The statement without its `PACK` clause, and the clause; `None` when it
/// has none. The clause is `PACK n TOKENS` and its options, outside any
/// parenthesis, and runs to the statement's end or its `LIMIT` or `OFFSET`.
pub fn split(sql_text: &str) -> Result<Option<(String, Spec)>> {
    if !may_pack(sql_text) {
        return Ok(None);
    }
    let Ok(tokens) = Tokenizer::new(&GenericDialect {}, sql_text).tokenize_with_location() else { return Ok(None) };
    let word = |t: &Token, w: &str| matches!(t, Token::Word(x) if x.quote_style.is_none() && x.value.eq_ignore_ascii_case(w));
    let solid: Vec<usize> = (0..tokens.len()).filter(|&i| !matches!(tokens[i].token, Token::Whitespace(_))).collect();
    let mut depth = 0usize;
    let mut start = None;
    for (n, &i) in solid.iter().enumerate() {
        match &tokens[i].token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            t if depth == 0 && word(t, "PACK") => {
                let after: Vec<&Token> = solid[n + 1..].iter().take(2).map(|&j| &tokens[j].token).collect();
                if let [Token::Number(..), t] = after[..] {
                    if word(t, "TOKENS") {
                        start = Some(n);
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    let Some(start) = start else { return Ok(None) };
    // The clause's tokens, to LIMIT, OFFSET, `;` or the end.
    let mut end = start;
    while end < solid.len() {
        let t = &tokens[solid[end]].token;
        if end > start && (word(t, "LIMIT") || word(t, "OFFSET") || *t == Token::SemiColon) {
            break;
        }
        end += 1;
    }
    let clause: Vec<&Token> = solid[start..end].iter().map(|&i| &tokens[i].token).collect();
    let spec = parse_clause(&clause)?;
    let from = offset_of(sql_text, tokens[solid[start]].span.start);
    let to = match solid.get(end) {
        Some(&i) => offset_of(sql_text, tokens[i].span.start),
        None => sql_text.len(),
    };
    let mut rest = String::with_capacity(sql_text.len());
    rest.push_str(sql_text[..from].trim_end());
    rest.push(' ');
    rest.push_str(&sql_text[to..]);
    Ok(Some((rest.trim().to_string(), spec)))
}

/// The byte offset of a token's location in `text` (lines and columns from
/// 1, columns counted in characters).
fn offset_of(text: &str, at: Location) -> usize {
    let mut line = 1;
    let mut col = 1;
    for (i, c) in text.char_indices() {
        if line == at.line && col == at.column {
            return i;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    text.len()
}

/// `PACK n TOKENS [OF a, b] [EXCERPT n] [DEDUPE x [ON c]] [FOLLOW f [n
/// HOP]] [WITH RECEIPT]`, the options in any order.
fn parse_clause(t: &[&Token]) -> Result<Spec> {
    const SHAPE: &str = "PACK n TOKENS [OF column, ...] [EXCERPT n] [DEDUPE x [ON column]] [FOLLOW field [n HOP]] [WITH RECEIPT]";
    let word = |i: usize, w: &str| matches!(t.get(i), Some(Token::Word(x)) if x.quote_style.is_none() && x.value.eq_ignore_ascii_case(w));
    let name = |i: usize| match t.get(i) {
        Some(Token::Word(x)) => Some(x.value.clone()),
        _ => None,
    };
    let number = |i: usize| match t.get(i) {
        Some(Token::Number(s, _)) => s.parse::<f64>().ok(),
        _ => None,
    };
    let whole = |i: usize, what: &str| -> Result<usize> {
        match number(i) {
            Some(x) if x >= 1.0 && x.fract() == 0.0 && x <= MAX_BUDGET as f64 => Ok(x as usize),
            _ => Err(err(format!("PACK: {what} takes a whole number, 1 or more (at most {MAX_BUDGET})"))),
        }
    };
    let mut spec = Spec { budget: whole(1, "PACK n TOKENS")?, of: Vec::new(), excerpt: None, dedupe: None, follow: None, receipt: false };
    let mut i = 3;
    while i < t.len() {
        if word(i, "OF") && spec.of.is_empty() {
            i += 1;
            loop {
                spec.of.push(name(i).ok_or_else(|| err("PACK ... OF takes the names of the columns whose text it counts"))?);
                i += 1;
                if t.get(i) != Some(&&Token::Comma) {
                    break;
                }
                i += 1;
            }
        } else if word(i, "EXCERPT") && spec.excerpt.is_none() {
            spec.excerpt = Some(whole(i + 1, "EXCERPT n")?.max(MIN_EXCERPT));
            i += 2;
        } else if word(i, "DEDUPE") && spec.dedupe.is_none() {
            let x = number(i + 1).filter(|x| *x > 0.0 && *x <= 1.0).ok_or_else(|| err("PACK ... DEDUPE x takes a share above 0 and at most 1 (0.85: rows of which 85% says what a row taken says)"))?;
            i += 2;
            let on = match word(i, "ON") {
                true => {
                    i += 2;
                    Some(name(i - 1).ok_or_else(|| err("PACK ... DEDUPE x ON takes a VECTOR column"))?)
                }
                false => None,
            };
            spec.dedupe = Some((x, on));
        } else if word(i, "FOLLOW") && spec.follow.is_none() {
            let field = name(i + 1).ok_or_else(|| err("PACK ... FOLLOW takes an EDGES field"))?;
            i += 2;
            let mut hops = 1;
            if number(i).is_some() && (word(i + 1, "HOP") || word(i + 1, "HOPS")) {
                hops = whole(i, "FOLLOW field n HOP")?;
                if hops > 3 {
                    return Err(err("PACK ... FOLLOW field n HOP follows at most 3 hops"));
                }
                i += 2;
            }
            spec.follow = Some((field, hops));
        } else if word(i, "WITH") && word(i + 1, "RECEIPT") && !spec.receipt {
            spec.receipt = true;
            i += 2;
        } else {
            let at = t.get(i).map(|x| x.to_string()).unwrap_or_default();
            return Err(err(format!("PACK: {at} is not an option here; the clause is {SHAPE}, and LIMIT or OFFSET follow it")));
        }
    }
    Ok(spec)
}

/// One candidate, as decided.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    /// The row's primary key (`Null` in a table without one).
    pub key: Value,
    /// The query's score of the row.
    pub score: f64,
    /// The tokens the row spends: of its excerpt when excerpted, 0 when
    /// skipped.
    pub tokens: usize,
    /// The tokens of the whole row.
    pub full_tokens: usize,
    /// The tokens spent up to and with this row.
    pub used: usize,
    /// `take`, `excerpt`, `linked from k`, `skip: near-duplicate of k` or
    /// `skip: over budget`.
    pub decision: String,
    pub taken: bool,
}

/// What [`Packer::finish`] answers: the rows, and how they were chosen.
#[derive(Clone, Debug)]
pub struct Packed {
    /// The rows taken (and with `WITH RECEIPT` the rows skipped), with the
    /// pack's columns.
    pub rs: ResultSet,
    pub receipt: Receipt,
}

/// How a pack was made.
#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    /// The snapshot's transaction ([`crate::storage::Snapshot::txn`]) and
    /// write number ([`crate::storage::Snapshot::seq_no`]); `None` where
    /// they are not known (a server's answer).
    pub txn: Option<u64>,
    pub seq_no: Option<u64>,
    pub budget: usize,
    pub used: usize,
    /// Every candidate decided, in the order it was decided.
    pub rows: Vec<Decision>,
    /// The queries run to read the candidates.
    pub queries: usize,
}

/// What the query that is running reads.
enum Pending {
    /// Rows `offset..offset + n` of the query.
    Slice { n: usize },
    /// The rows the row with this key links to.
    Follow { from: String },
}

/// A candidate waiting for its decision.
struct Candidate {
    row: Vec<Value>,
    key: Value,
    score: f64,
}

/// What a taken row is compared with for `DEDUPE`.
struct Kept {
    label: String,
    words: HashSet<String>,
    vector: Option<Vec<f32>>,
}

/// Runs a `PACK` query: ask it for the next query to run
/// ([`Packer::next_query`]), give it that query's answer ([`Packer::feed`]),
/// until it has none, then [`Packer::finish`].
pub struct Packer {
    spec: Spec,
    /// The query without its `PACK` clause, `LIMIT` and `OFFSET`.
    query: sql::Query,
    /// The VECTOR column `DEDUPE x ON` reads, added after the query's
    /// columns.
    hidden: usize,
    /// The words of the query's search texts, which excerpts are cut around.
    terms: Vec<String>,
    next_offset: usize,
    end: usize,
    slice: usize,
    exhausted: bool,
    pending: Option<Pending>,
    buffer: VecDeque<Candidate>,
    follows: VecDeque<(Value, String)>,
    seen: HashSet<String>,
    columns: Option<Vec<String>>,
    text_cols: Vec<usize>,
    kept: Vec<Kept>,
    used: usize,
    out_rows: Vec<Vec<Value>>,
    out_keys: Vec<Value>,
    out_scores: Vec<f64>,
    decisions: Vec<Decision>,
    queries: usize,
    stats: Option<ExecStats>,
}

impl Packer {
    /// The packer of `sql_text`, `None` when it has no `PACK` clause.
    pub fn of_sql(sql_text: &str, params: &Params) -> Result<Option<Packer>> {
        let Some((rest, spec)) = split(sql_text)? else { return Ok(None) };
        let q = *super::sql::parse_query(&rest)?;
        Packer::new(q, spec, params).map(Some)
    }

    /// The packer of query `q` (without its `PACK` clause) by `spec`.
    pub fn new(mut q: sql::Query, spec: Spec, params: &Params) -> Result<Packer> {
        let number = |e: &sql::Expr, what: &str| -> Result<usize> {
            let v = match e {
                sql::Expr::Value(v) => match &v.value {
                    sql::Value::Number(s, _) => s.parse::<i64>().ok().map(Value::Int),
                    sql::Value::Placeholder(p) => params.0.get(p.trim_start_matches([':', '$', '@', '?'])).cloned(),
                    _ => None,
                },
                _ => None,
            };
            match v {
                Some(Value::Int(n)) if n >= 0 => Ok(n as usize),
                _ => Err(err(format!("PACK: {what} takes a whole number of rows"))),
            }
        };
        let (limit, offset) = match q.limit_clause.take() {
            None => (None, 0),
            Some(sql::LimitClause::LimitOffset { limit, offset, limit_by }) if limit_by.is_empty() => {
                (limit.as_ref().map(|l| number(l, "LIMIT")).transpose()?, offset.as_ref().map(|o| number(&o.value, "OFFSET")).transpose()?.unwrap_or(0))
            }
            Some(sql::LimitClause::OffsetCommaLimit { offset, limit }) => (Some(number(&limit, "LIMIT")?), number(&offset, "OFFSET")?),
            Some(_) => return Err(err("PACK: LIMIT BY is not supported")),
        };
        if q.fetch.is_some() {
            return Err(err("PACK reads the rows up to LIMIT n: not with FETCH"));
        }
        let candidates = limit.unwrap_or(DEFAULT_CANDIDATES);
        let mut hidden = 0;
        if let Some((_, Some(on))) = &spec.dedupe {
            let sql::SetExpr::Select(s) = q.body.as_mut() else { return Err(err("PACK ... DEDUPE x ON column reads the column from a SELECT of one table")) };
            s.projection.push(sql::SelectItem::ExprWithAlias { expr: sql::Expr::Identifier(sql::Ident::new(on)), alias: sql::Ident::new("__pack_vector") });
            hidden = 1;
        }
        if spec.follow.is_some() && !matches!(q.body.as_ref(), sql::SetExpr::Select(_)) {
            return Err(err("PACK ... FOLLOW follows the edges of a SELECT of one table"));
        }
        let terms = search_terms(&q, params);
        Ok(Packer {
            spec,
            query: q,
            hidden,
            terms,
            next_offset: offset,
            end: offset.saturating_add(candidates),
            slice: FIRST_SLICE,
            exhausted: candidates == 0,
            pending: None,
            buffer: VecDeque::new(),
            follows: VecDeque::new(),
            seen: HashSet::new(),
            columns: None,
            text_cols: Vec::new(),
            kept: Vec::new(),
            used: 0,
            out_rows: Vec::new(),
            out_keys: Vec::new(),
            out_scores: Vec::new(),
            decisions: Vec::new(),
            queries: 0,
            stats: None,
        })
    }

    pub fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Whether what is left of the budget is too small for any row.
    fn full(&self) -> bool {
        self.spec.budget.saturating_sub(self.used) < MIN_EXCERPT.min(self.spec.budget)
    }

    /// The text of the next query to run, `None` when the pack is made.
    pub fn next_query(&mut self) -> Result<Option<String>> {
        if self.pending.is_some() {
            return Err(err("PACK: the answer of the last query was not given"));
        }
        self.drive()?;
        if self.full() {
            return Ok(None);
        }
        if let Some((key, from)) = self.follows.pop_front() {
            let (field, hops) = self.spec.follow.clone().expect("follows come from FOLLOW");
            let mut q = self.query.clone();
            let sql::SetExpr::Select(s) = q.body.as_mut() else { unreachable!("checked in new") };
            let reach = format!("REACHABLE({}, {}, hops => {hops})", sql::Ident::new(&field), literal(&key)?);
            let reach = sqlparser::parser::Parser::new(&GenericDialect {}).try_with_sql(&reach).and_then(|mut p| p.parse_expr()).map_err(|e| err(format!("PACK ... FOLLOW: {e}")))?;
            s.selection = Some(match s.selection.take() {
                Some(w) => sql::Expr::BinaryOp { left: Box::new(sql::Expr::Nested(Box::new(w))), op: sql::BinaryOperator::And, right: Box::new(reach) },
                None => reach,
            });
            // Not in the query's order: a ranking by bm25 would keep only
            // the linked rows that match its words, and the link is the
            // reason to take them.
            q.order_by = None;
            q.limit_clause = Some(limit_offset(FOLLOWED, 0));
            self.pending = Some(Pending::Follow { from });
            self.queries += 1;
            return Ok(Some(q.to_string()));
        }
        if self.exhausted || self.next_offset >= self.end {
            return Ok(None);
        }
        let n = self.slice.min(self.end - self.next_offset);
        let mut q = self.query.clone();
        q.limit_clause = Some(limit_offset(n, self.next_offset));
        self.pending = Some(Pending::Slice { n });
        self.queries += 1;
        Ok(Some(q.to_string()))
    }

    /// Gives the packer the answer of the query [`Packer::next_query`] gave.
    pub fn feed(&mut self, rs: ResultSet) -> Result<()> {
        let pending = self.pending.take().ok_or_else(|| err("PACK: an answer no query was asked for"))?;
        self.columns_of(&rs)?;
        if self.stats.is_none() {
            self.stats = Some(rs.stats.clone());
        }
        let n_rows = rs.rows.len();
        let mut cands: Vec<Candidate> = Vec::with_capacity(n_rows);
        for (i, row) in rs.rows.into_iter().enumerate() {
            let key = rs.keys.get(i).cloned().unwrap_or(Value::Null);
            let score = rs.scores.get(i).copied().unwrap_or(0.0);
            cands.push(Candidate { row, key, score });
        }
        match pending {
            Pending::Slice { n } => {
                self.next_offset += n;
                if n_rows < n {
                    self.exhausted = true;
                }
                self.slice = self.slice.saturating_mul(4);
                self.buffer.extend(cands);
            }
            Pending::Follow { from } => {
                for c in cands {
                    if self.full() {
                        break;
                    }
                    self.decide(c, Some(&from))?;
                }
            }
        }
        Ok(())
    }

    /// Decides the candidates read, up to a row taken that has edges to
    /// follow.
    fn drive(&mut self) -> Result<()> {
        while self.follows.is_empty() && !self.full() {
            let Some(c) = self.buffer.pop_front() else { break };
            self.decide(c, None)?;
        }
        Ok(())
    }

    /// The columns of the first answer, and which hold the text counted.
    fn columns_of(&mut self, rs: &ResultSet) -> Result<()> {
        if self.columns.is_some() {
            return Ok(());
        }
        let shown = rs.columns.len().saturating_sub(self.hidden);
        let columns: Vec<String> = rs.columns[..shown].to_vec();
        self.text_cols = match self.spec.of.is_empty() {
            false => self
                .spec
                .of
                .iter()
                .map(|c| columns.iter().position(|x| x.eq_ignore_ascii_case(c)).ok_or_else(|| err(format!("PACK ... OF {c}: the query has no column {c} (its columns: {})", columns.join(", ")))))
                .collect::<Result<_>>()?,
            // Every column of text, but one that holds the row's key.
            true => (0..shown).filter(|&j| rs.rows.iter().any(|r| matches!(r.get(j), Some(Value::Str(_))))).filter(|&j| !rs.rows.iter().enumerate().all(|(i, r)| rs.keys.get(i).is_some_and(|k| !matches!(k, Value::Null) && r.get(j) == Some(k)))).collect(),
        };
        if self.text_cols.is_empty() && !rs.rows.is_empty() {
            return Err(err(format!("PACK counts the tokens of the query's text columns, and it has none (its columns: {}); select one, or name them with OF", columns.join(", "))));
        }
        self.columns = Some(columns);
        Ok(())
    }

    /// Decides one candidate.
    fn decide(&mut self, c: Candidate, linked_from: Option<&str>) -> Result<()> {
        let label = key_label(&c.key, self.decisions.len());
        if !matches!(c.key, Value::Null) && !self.seen.insert(label.clone()) {
            return Ok(());
        }
        let Candidate { mut row, key, score } = c;
        let vector = match self.hidden {
            0 => None,
            _ => match row.pop() {
                Some(Value::Vector(v)) => Some(v.to_vec()),
                Some(Value::Null) | None => None,
                Some(_) => return Err(err("PACK ... DEDUPE x ON column compares vectors: name a VECTOR column")),
            },
        };
        let text_of = |row: &[Value], j: usize| match row.get(j) {
            Some(Value::Str(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => plain(v),
        };
        let counts: Vec<usize> = self.text_cols.iter().map(|&j| count_tokens(&text_of(&row, j))).collect();
        let full: usize = counts.iter().sum();
        let left = self.spec.budget - self.used;
        let room = left.min(self.spec.row_cap());
        // Whole, or an excerpt of the longest text column.
        let (tokens, excerpted) = if full <= room {
            (full, false)
        } else {
            let (longest, &most) = counts.iter().enumerate().max_by_key(|(_, n)| **n).expect("a text column");
            let rest = full - most;
            if rest + MIN_EXCERPT > room || room < MIN_EXCERPT {
                (0, false)
            } else {
                let j = self.text_cols[longest];
                let cut = excerpt(&text_of(&row, j), &self.terms, room - rest);
                let n = count_tokens(&cut);
                row[j] = Value::Str(cut);
                (rest + n, true)
            }
        };
        let words: HashSet<String> = self.text_cols.iter().flat_map(|&j| word_set(&text_of(&row, j))).collect();
        let decision = if tokens == 0 {
            "skip: over budget".to_string()
        } else if let Some(dup) = self.duplicate_of(&words, vector.as_deref()) {
            format!("skip: near-duplicate of {dup}")
        } else {
            match (linked_from, excerpted) {
                (Some(from), false) => format!("linked from {from}"),
                (Some(from), true) => format!("excerpt, linked from {from}"),
                (None, false) => "take".to_string(),
                (None, true) => "excerpt".to_string(),
            }
        };
        let taken = !decision.starts_with("skip");
        let spent = if taken { tokens } else { 0 };
        self.used += spent;
        if taken {
            self.kept.push(Kept { label: label.clone(), words, vector });
            if self.spec.follow.is_some() && linked_from.is_none() {
                if matches!(key, Value::Null) {
                    return Err(err("PACK ... FOLLOW follows the edges of rows by their keys: the table needs a PRIMARY KEY"));
                }
                self.follows.push_back((key.clone(), label.clone()));
            }
        }
        self.decisions.push(Decision { key: key.clone(), score, tokens: spent, full_tokens: full, used: self.used, decision: decision.clone(), taken });
        if taken || self.spec.receipt {
            row.push(Value::Int(spent as i64));
            row.push(Value::Str(decision));
            if self.spec.receipt {
                row.push(Value::Float(score));
                row.push(Value::Int(self.used as i64));
            }
            self.out_rows.push(row);
            self.out_keys.push(key);
            self.out_scores.push(score);
        }
        Ok(())
    }

    /// The label of a row taken that the row of these words and vector
    /// repeats, by `DEDUPE`.
    fn duplicate_of(&self, words: &HashSet<String>, vector: Option<&[f32]>) -> Option<String> {
        let (x, on) = self.spec.dedupe.as_ref()?;
        self.kept
            .iter()
            .find(|k| match (on, vector, k.vector.as_deref()) {
                (Some(_), Some(a), Some(b)) => cosine(a, b) >= *x,
                (Some(_), _, _) => false,
                (None, _, _) => overlap(words, &k.words) >= *x,
            })
            .map(|k| k.label.clone())
    }

    /// The pack: the rows taken and its receipt, read from the snapshot of
    /// `txn` and `seq_no` where they are known.
    pub fn finish(mut self, txn: Option<u64>, seq_no: Option<u64>) -> Result<Packed> {
        if self.pending.is_some() {
            return Err(err("PACK: the answer of the last query was not given"));
        }
        // Rows read and not decided stay undecided: the budget was full.
        let mut columns = self.columns.take().unwrap_or_default();
        columns.push("pack_tokens".into());
        columns.push("pack_decision".into());
        if self.spec.receipt {
            columns.push("pack_score".into());
            columns.push("pack_used".into());
        }
        let mut stats = self.stats.take().unwrap_or_default();
        stats.plan.push(format!(
            "pack: {} of {} tokens in {} rows, {} candidates decided, read by {} quer{}",
            self.used,
            self.spec.budget,
            self.decisions.iter().filter(|d| d.taken).count(),
            self.decisions.len(),
            self.queries,
            if self.queries == 1 { "y" } else { "ies" }
        ));
        let receipt = Receipt { txn, seq_no, budget: self.spec.budget, used: self.used, rows: self.decisions, queries: self.queries };
        let rs = ResultSet { columns, rows: self.out_rows, keys: self.out_keys, scores: self.out_scores, parts: None, stats };
        Ok(Packed { rs, receipt })
    }
}

/// `LIMIT n OFFSET o`.
fn limit_offset(n: usize, offset: usize) -> sql::LimitClause {
    let num = |n: usize| sql::Expr::value(sql::Value::Number(n.to_string(), false));
    sql::LimitClause::LimitOffset { limit: Some(num(n)), offset: (offset > 0).then(|| sql::Offset { value: num(offset), rows: sql::OffsetRows::None }), limit_by: Vec::new() }
}

/// A key as an SQL literal.
fn literal(v: &Value) -> Result<String> {
    match v {
        Value::Int(i) => Ok(i.to_string()),
        Value::Str(s) => Ok(format!("'{}'", s.replace('\'', "''"))),
        v => Err(err(format!("PACK ... FOLLOW: a key of {} cannot be followed", super::hint::kind_of(v)))),
    }
}

/// How a row is named in decisions: its key, else its place.
fn key_label(key: &Value, n: usize) -> String {
    match key {
        Value::Null => format!("#{}", n + 1),
        Value::Str(s) => s.clone(),
        v => plain(v),
    }
}

/// A value that is not text, as text.
fn plain(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Int(i) => i.to_string(),
        Value::Float(x) => x.to_string(),
        Value::Str(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Timestamp(t) => crate::timestamp::format(*t),
        Value::Vector(v) => format!("{} numbers", v.len()),
        Value::List(xs) => xs.iter().map(plain).collect::<Vec<_>>().join(", "),
    }
}

/// The words of `text` that say something: lowercased, two letters or more.
fn word_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(|w| w.to_lowercase()).collect()
}

/// The share of the smaller set's words that are in the other.
fn overlap(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let (small, big) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if small.is_empty() {
        return 0.0;
    }
    small.iter().filter(|w| big.contains(*w)).count() as f64 / small.len() as f64
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        ab += (*x as f64) * (*y as f64);
        aa += (*x as f64) * (*x as f64);
        bb += (*y as f64) * (*y as f64);
    }
    if aa == 0.0 || bb == 0.0 {
        return 0.0;
    }
    ab / (aa.sqrt() * bb.sqrt())
}

/// Words too common to cut an excerpt around.
const STOP: [&str; 24] = ["the", "and", "for", "with", "that", "this", "from", "are", "was", "were", "not", "but", "you", "your", "use", "how", "what", "why", "when", "can", "has", "have", "its", "into"];

/// The words of the texts the query searches by: the second argument of
/// each `bm25` and `MATCH` in its `WHERE` and `ORDER BY`.
fn search_terms(q: &sql::Query, params: &Params) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(sql::OrderBy { kind: sql::OrderByKind::Expressions(es), .. }) = &q.order_by {
        for e in es {
            texts_in(&e.expr, params, &mut texts);
        }
    }
    if let sql::SetExpr::Select(s) = q.body.as_ref() {
        if let Some(w) = &s.selection {
            texts_in(w, params, &mut texts);
        }
    }
    let mut terms: Vec<String> = Vec::new();
    for t in texts {
        for w in t.split(|c: char| !c.is_alphanumeric()) {
            let w = w.to_lowercase();
            if w.chars().count() >= 3 && !STOP.contains(&w.as_str()) && !terms.contains(&w) {
                terms.push(w);
            }
        }
    }
    terms
}

fn texts_in(e: &sql::Expr, params: &Params, out: &mut Vec<String>) {
    match e {
        sql::Expr::Function(f) => {
            let name = f.name.to_string();
            let sql::FunctionArguments::List(list) = &f.args else { return };
            let args: Vec<&sql::Expr> = list
                .args
                .iter()
                .filter_map(|a| match a {
                    sql::FunctionArg::Unnamed(sql::FunctionArgExpr::Expr(x)) => Some(x),
                    _ => None,
                })
                .collect();
            if name.eq_ignore_ascii_case("bm25") || name.eq_ignore_ascii_case("match") {
                if let Some(sql::Expr::Value(v)) = args.get(1) {
                    match &v.value {
                        sql::Value::SingleQuotedString(s) | sql::Value::DoubleQuotedString(s) => out.push(s.clone()),
                        sql::Value::Placeholder(p) => {
                            if let Some(Value::Str(s)) = params.0.get(p.trim_start_matches([':', '$', '@', '?'])) {
                                out.push(s.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            for a in args {
                texts_in(a, params, out);
            }
        }
        sql::Expr::BinaryOp { left, right, .. } => {
            texts_in(left, params, out);
            texts_in(right, params, out);
        }
        sql::Expr::Nested(x) | sql::Expr::UnaryOp { expr: x, .. } => texts_in(x, params, out),
        _ => {}
    }
}

/// Whether a word of a row's text is one of the query's terms (or shares
/// its stem: the first letters of the shorter, all but two, at least four).
fn matches_term(word: &str, terms: &[String]) -> bool {
    let w = word.to_lowercase();
    terms.iter().any(|t| {
        if w == *t {
            return true;
        }
        let stem = |s: &str| s.chars().count().saturating_sub(2).max(4);
        let n = stem(t).min(stem(&w));
        w.chars().count() >= 4 && t.chars().count() >= 4 && w.chars().take(n).eq(t.chars().take(n))
    })
}

/// The stretch of `text` of at most `budget` tokens (with the marks that
/// say it was cut) that holds the most of `terms`, the earliest of those;
/// the start of the text when it holds none.
pub fn excerpt(text: &str, terms: &[String], budget: usize) -> String {
    // Words with their byte ranges and tokens.
    let mut words: Vec<(usize, usize, usize, bool)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                let w = &text[s..i];
                let hit = w.split(|c: char| !c.is_alphanumeric()).any(|p| !p.is_empty() && matches_term(p, terms));
                words.push((s, i, count_tokens(w).max(1), hit));
                start = None;
            }
            _ => {}
        }
    }
    if words.is_empty() {
        return String::new();
    }
    let total: usize = words.iter().map(|w| w.2).sum();
    if total <= budget {
        return text.to_string();
    }
    // Two tokens for the marks (an ellipsis each side).
    let room = budget.saturating_sub(2).max(1);
    // The window of words of at most `room` tokens with the most hits.
    let (mut best, mut best_at, mut best_end) = (0usize, 0usize, 0usize);
    let (mut lo, mut tokens, mut hits) = (0usize, 0usize, 0usize);
    for hi in 0..words.len() {
        tokens += words[hi].2;
        hits += words[hi].3 as usize;
        while tokens > room && lo <= hi {
            tokens -= words[lo].2;
            hits -= words[lo].3 as usize;
            lo += 1;
        }
        if lo <= hi && hits > best {
            (best, best_at, best_end) = (hits, lo, hi + 1);
        }
    }
    if best == 0 {
        // No term: the text's start.
        best_at = 0;
        let mut t = 0;
        best_end = 0;
        while best_end < words.len() && t + words[best_end].2 <= room {
            t += words[best_end].2;
            best_end += 1;
        }
        best_end = best_end.max(1);
    } else {
        // Centre the hits: from the first to the last hit of the window,
        // grow it a word at a time on either side, after then before, while
        // the words fit.
        let first_hit = (best_at..best_end).find(|&i| words[i].3).unwrap_or(best_at);
        let last_hit = (best_at..best_end).rev().find(|&i| words[i].3).unwrap_or(best_end - 1);
        let (mut lo, mut hi) = (first_hit, last_hit + 1);
        let mut used: usize = words[lo..hi].iter().map(|w| w.2).sum();
        loop {
            let mut grew = false;
            if hi < words.len() && used + words[hi].2 <= room {
                used += words[hi].2;
                hi += 1;
                grew = true;
            }
            if lo > 0 && used + words[lo - 1].2 <= room {
                used += words[lo - 1].2;
                lo -= 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        (best_at, best_end) = (lo, hi);
    }
    let mut out = String::new();
    if best_at > 0 {
        out.push_str("… ");
    }
    out.push_str(&text[words[best_at].0..words[best_end - 1].1]);
    if best_end < words.len() {
        out.push_str(" …");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_tokens_near_bpe() {
        assert_eq!(count_tokens(""), 0);
        assert_eq!(count_tokens("The quick brown fox jumps over the lazy dog."), 10);
        assert_eq!(count_tokens("idempotency"), 2);
        assert_eq!(count_tokens("2026"), 2);
        assert_eq!(count_tokens("fn main() {}"), 6);
        assert_eq!(count_tokens("日本語"), 3);
    }

    #[test]
    fn splits_the_clause_off() {
        let (rest, spec) = split("SELECT id, text FROM m ORDER BY bm25(text, :q) DESC PACK 4000 TOKENS DEDUPE 0.85 FOLLOW links 1 HOP WITH RECEIPT LIMIT 200").unwrap().unwrap();
        assert_eq!(rest, "SELECT id, text FROM m ORDER BY bm25(text, :q) DESC LIMIT 200");
        assert_eq!(spec, Spec { budget: 4000, of: vec![], excerpt: None, dedupe: Some((0.85, None)), follow: Some(("links".into(), 1)), receipt: true });
        let (rest, spec) = split("SELECT a FROM t pack 100 tokens of a, b excerpt 50 dedupe 0.9 on emb;").unwrap().unwrap();
        assert_eq!(rest, "SELECT a FROM t ;");
        assert_eq!(spec.of, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(spec.excerpt, Some(50));
        assert_eq!(spec.dedupe, Some((0.9, Some("emb".into()))));
        // A column named pack, and a PACK inside parentheses, are not the clause.
        assert!(split("SELECT pack FROM t WHERE pack > 3").unwrap().is_none());
        assert!(split("SELECT id FROM t WHERE id IN (SELECT id FROM t PACK 10 TOKENS)").unwrap().is_none());
        assert!(split("SELECT a FROM t PACK 0 TOKENS").is_err());
        assert!(split("SELECT a FROM t PACK 10 TOKENS DEDUPE 2").is_err());
        assert!(split("SELECT a FROM t PACK 10 TOKENS SOMETHING").unwrap_err().to_string().contains("not an option"));
    }

    #[test]
    fn excerpts_around_the_terms() {
        let filler = "lorem ipsum dolor sit amet ".repeat(40);
        let text = format!("{filler}the retry backoff hit its cap of five seconds {filler}");
        let cut = excerpt(&text, &["backoff".into(), "retry".into()], 20);
        assert!(cut.contains("retry backoff"), "{cut}");
        assert!(cut.starts_with("… ") && cut.ends_with(" …"), "{cut}");
        assert!(count_tokens(&cut) <= 20, "{} tokens: {cut}", count_tokens(&cut));
        // No term: the start.
        let cut = excerpt(&text, &[], 10);
        assert!(cut.starts_with("lorem") && count_tokens(&cut) <= 10);
        // Short text comes back whole.
        assert_eq!(excerpt("a short note", &[], 10), "a short note");
    }

    #[test]
    fn overlap_of_words() {
        let a = word_set("use exponential backoff capped at 5 s");
        let b = word_set("Decision: use exponential backoff, capped at 5 s, with jitter");
        assert!(overlap(&a, &b) >= 0.85);
        assert!(overlap(&a, &word_set("payments retry storm on Tuesday")) < 0.2);
    }
}
