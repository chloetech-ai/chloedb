//! Classifiers: a linear model over a VECTOR field's embeddings, trained
//! from a table's labelled rows and kept as a stored function
//! ([`super::function`]), so it is versioned, rolled back and dropped as a
//! function is, and every shard of a cluster holds it.
//!
//! ```sql
//! CREATE CLASSIFIER topic ON products(emb) LABELS category
//!   OPTIONS (method = 'logistic', holdout = 0.2) WHERE category IS NOT NULL;
//! -- writes: CREATE FUNCTION topic(v VECTOR, returns TEXT DEFAULT 'label')
//! --   RETURNS KEYWORD AS classify(:v, '<the model>', returns => :returns)
//!
//! SELECT title, topic(emb) FROM products LIMIT 10;             -- each row's class
//! SELECT topic(:q), topic(:q, 'score');                         -- a query's, :q embedded
//! SELECT title FROM products WHERE category = topic(:q) ...     -- folded before planning
//! ```
//!
//! * **Training.** `CREATE CLASSIFIER` reads the rows (`SELECT field,
//!   labels FROM table WHERE ...`, at most `max_rows` of them), keeps the
//!   labels with at least `min_rows` rows, and holds out a share of them
//!   (`holdout`, 0.2; chosen by a hash of the vector, so the same rows
//!   every time) to measure the model it keeps. `method = 'logistic'`
//!   (the default) fits softmax regression with Adam; `'centroid'` takes
//!   each label's mean vector, which trains in one pass and needs fewer
//!   rows a label. The statement answers a row of what it measured.
//! * **The model** is the function's body: the labels, a row of weights a
//!   label quantized to 8 bits with its own scale, and a bias, in JSON
//!   with the weights in base64 ([`Classifier::encode`]). It records the
//!   table, field and embedding model it was trained on, so text is
//!   embedded the same way ([`crate::Db::classify`], `POST /v1/classify`).
//! * **`classify(v, model)`** gives the label with the highest probability
//!   (softmax of the scores), or `NULL` when that probability is below the
//!   model's `min_score` or `v` is `NULL`; `returns => 'score'` gives the
//!   probability, `returns => 'labels'` the best label and the others
//!   whose probability is at least `min_score` (and a tenth), best first. With a constant
//!   argument (a parameter, a literal) a call of the label or the score is
//!   computed before the query is planned and may stand anywhere; over a row's vector it is a column
//!   of the rows a query returns, or ranks them, as `llm()` is
//!   ([`super::llm`]), computed after the query finds them. To filter or
//!   group rows by a class, keep it in a column: a `KEYWORD` field derived
//!   with model `classifier:<name>` ([`crate::content::ClassifierTags`])
//!   labels each row as it is written.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, VisitMut, VisitorMut};
use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Location, Token, TokenWithSpan, Tokenizer};

use super::exec::{ExecStats, ResultSet};
use super::Params;
use crate::error::{Error, Result};
use crate::schema::{FieldType, Schema};
use crate::value::Value;

/// The function's name.
pub const NAME: &str = "classify";

/// How `CREATE CLASSIFIER` is written.
pub const CREATE_FORM: &str = "CREATE [OR REPLACE] CLASSIFIER name ON table(vector_field) LABELS column [OPTIONS (method = 'logistic' | 'centroid', holdout = 0.2, min_score = 0, min_rows = 2, max_rows = 200000, epochs = 30)] [WHERE condition]";
/// How `DROP CLASSIFIER` is written.
pub const DROP_FORM: &str = "DROP CLASSIFIER [IF EXISTS] name";

const USAGE: &str = "classify(vector, model [, returns => 'label' | 'score' | 'labels']): a model CREATE CLASSIFIER made, as its function calls it";

/// Rows a training reads by default, at most.
pub const MAX_ROWS: usize = 200_000;
/// The labels a classifier has, at most.
pub const MAX_LABELS: usize = 10_000;
/// The rows a query computes a call over a row's vector for, at most.
pub const MAX_CALL_ROWS: usize = 100_000;
/// The probability a label needs, at least, to be one of the labels
/// `returns => 'labels'` gives (besides the best).
pub const LABELS_FLOOR: f32 = 0.1;
/// Models decoded and kept in memory, by their text's hash.
const CACHED: usize = 32;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// How a classifier is fitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Softmax regression.
    Logistic,
    /// Each label's mean vector; scores are cosines times [`CENTROID_SCALE`].
    Centroid,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::Logistic => "logistic",
            Method::Centroid => "centroid",
        }
    }
}

/// What a centroid model multiplies cosines by, so that its softmax says
/// something: a cosine apart by 0.1 is e^2 times as likely.
pub const CENTROID_SCALE: f32 = 20.0;

/// The options of `CREATE CLASSIFIER`.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub method: Method,
    pub holdout: f64,
    pub min_score: f32,
    pub min_rows: usize,
    pub max_rows: usize,
    pub epochs: usize,
    /// L2 penalty of the weights (logistic).
    pub l2: f32,
    pub learning_rate: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options { method: Method::Logistic, holdout: 0.2, min_score: 0.0, min_rows: 2, max_rows: MAX_ROWS, epochs: 30, l2: 1e-4, learning_rate: 0.01 }
    }
}

/// `CREATE CLASSIFIER` or `DROP CLASSIFIER`.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Create(Create),
    Drop { name: String, if_exists: bool },
}

impl Statement {
    /// The stored function statement a `DROP CLASSIFIER` is.
    pub fn drop_sql(&self) -> Option<String> {
        match self {
            Statement::Drop { name, if_exists } => Some(format!("DROP FUNCTION {}{name}", if *if_exists { "IF EXISTS " } else { "" })),
            Statement::Create(_) => None,
        }
    }
}

/// A `CREATE CLASSIFIER`.
#[derive(Clone, Debug, PartialEq)]
pub struct Create {
    pub name: String,
    pub replace: bool,
    pub table: String,
    pub field: String,
    pub labels: String,
    pub options: Options,
    /// The text after `WHERE`.
    pub condition: Option<String>,
}

fn byte_at(text: &str, at: Location) -> usize {
    let mut line_start = 0;
    for _ in 1..at.line {
        line_start += text[line_start..].find('\n').map_or(text.len() - line_start, |n| n + 1);
    }
    line_start + text[line_start..].chars().take((at.column as usize).saturating_sub(1)).map(char::len_utf8).sum::<usize>()
}

/// `CREATE [OR REPLACE] CLASSIFIER` or `DROP CLASSIFIER` of text `text`;
/// `None` for any other statement.
pub fn parse_statement(text: &str) -> Result<Option<Statement>> {
    let head = text.trim_start();
    let first: String = head.chars().take(6).collect::<String>().to_ascii_uppercase();
    if !(first.starts_with("CREATE") || first.starts_with("DROP")) || !text.as_bytes().windows(10).any(|w| w.eq_ignore_ascii_case(b"CLASSIFIER")) {
        return Ok(None);
    }
    let all = match Tokenizer::new(&GenericDialect {}, text).tokenize_with_location() {
        Ok(t) => t,
        Err(e) => {
            let words: Vec<String> = head.split_whitespace().take(4).map(str::to_ascii_uppercase).collect();
            if words.iter().any(|w| w == "CLASSIFIER") {
                return Err(err(format!("SQL syntax error: {e}")));
            }
            return Ok(None);
        }
    };
    let mut toks: Vec<&TokenWithSpan> = all.iter().filter(|t| !matches!(t.token, Token::Whitespace(_))).collect();
    if toks.last().is_some_and(|t| t.token == Token::SemiColon) {
        toks.pop();
    }
    let word = |i: usize| match toks.get(i).map(|t| &t.token) {
        Some(Token::Word(w)) if w.quote_style.is_none() => w.value.to_ascii_uppercase(),
        _ => String::new(),
    };
    let name_at = |i: usize| match toks.get(i).map(|t| &t.token) {
        Some(Token::Word(w)) => Some(w.value.clone()),
        _ => None,
    };
    let create = match word(0).as_str() {
        "CREATE" => true,
        "DROP" => false,
        _ => return Ok(None),
    };
    let replace = create && word(1) == "OR" && word(2) == "REPLACE";
    let mut i = if replace { 3 } else { 1 };
    if word(i) != "CLASSIFIER" {
        return Ok(None);
    }
    i += 1;
    let form = if create { CREATE_FORM } else { DROP_FORM };
    let bad = |m: &str| err(format!("{m}; the form is {form}"));
    if !create {
        let if_exists = word(i) == "IF" && word(i + 1) == "EXISTS";
        if if_exists {
            i += 2;
        }
        let name = name_at(i).ok_or_else(|| bad("expected the classifier's name"))?.to_ascii_lowercase();
        if i + 1 != toks.len() {
            return Err(bad(&format!("unexpected {}", toks[i + 1].token)));
        }
        return Ok(Some(Statement::Drop { name, if_exists }));
    }
    let name = name_at(i).ok_or_else(|| bad("expected the classifier's name"))?.to_ascii_lowercase();
    i += 1;
    if word(i) != "ON" {
        return Err(bad("expected ON table(vector_field)"));
    }
    let table = name_at(i + 1).ok_or_else(|| bad("expected the table after ON"))?;
    if toks.get(i + 2).map(|t| &t.token) != Some(&Token::LParen) || toks.get(i + 4).map(|t| &t.token) != Some(&Token::RParen) {
        return Err(bad("expected the vector field in parentheses after the table"));
    }
    let field = name_at(i + 3).ok_or_else(|| bad("expected the vector field"))?;
    i += 5;
    if word(i) != "LABELS" {
        return Err(bad("expected LABELS column"));
    }
    let labels = name_at(i + 1).ok_or_else(|| bad("expected the column of labels after LABELS"))?;
    i += 2;
    let mut options = Options::default();
    if word(i) == "OPTIONS" {
        if toks.get(i + 1).map(|t| &t.token) != Some(&Token::LParen) {
            return Err(bad("expected OPTIONS (name = value, ...)"));
        }
        i += 2;
        loop {
            let key = word(i).to_ascii_lowercase();
            if key.is_empty() || toks.get(i + 1).map(|t| &t.token) != Some(&Token::Eq) {
                return Err(bad("expected an option, name = value"));
            }
            let value = match toks.get(i + 2).map(|t| &t.token) {
                Some(Token::Number(n, _)) => n.clone(),
                Some(Token::SingleQuotedString(s)) => s.clone(),
                Some(Token::Word(w)) => w.value.clone(),
                _ => return Err(bad(&format!("expected a value for {key}"))),
            };
            set_option(&mut options, &key, &value)?;
            i += 3;
            match toks.get(i).map(|t| &t.token) {
                Some(Token::Comma) => i += 1,
                Some(Token::RParen) => {
                    i += 1;
                    break;
                }
                _ => return Err(bad("expected , or ) after an option")),
            }
        }
    }
    let condition = match word(i).as_str() {
        "WHERE" => {
            let start = toks.get(i + 1).ok_or_else(|| bad("expected a condition after WHERE"))?;
            let at = byte_at(text, start.span.start);
            let c = text[at..].trim().trim_end_matches(';').trim().to_string();
            Some(c)
        }
        "" if i == toks.len() => None,
        _ => return Err(bad(&format!("unexpected {}", toks[i].token))),
    };
    Ok(Some(Statement::Create(Create { name, replace, table, field, labels, options, condition })))
}

fn set_option(o: &mut Options, key: &str, value: &str) -> Result<()> {
    let num = |what: &str| -> Result<f64> { value.parse::<f64>().map_err(|_| err(format!("CREATE CLASSIFIER: {key} is {what}, not {value:?}"))) };
    match key {
        "method" => {
            o.method = match value.to_ascii_lowercase().as_str() {
                "logistic" => Method::Logistic,
                "centroid" => Method::Centroid,
                _ => return Err(err(format!("CREATE CLASSIFIER: method is 'logistic' or 'centroid', not {value:?}"))),
            }
        }
        "holdout" => {
            let h = num("a share from 0 to 0.9")?;
            if !(0.0..=0.9).contains(&h) {
                return Err(err("CREATE CLASSIFIER: holdout is a share from 0 to 0.9"));
            }
            o.holdout = h;
        }
        "min_score" => {
            let s = num("a probability from 0 to 1")?;
            if !(0.0..=1.0).contains(&s) {
                return Err(err("CREATE CLASSIFIER: min_score is a probability from 0 to 1"));
            }
            o.min_score = s as f32;
        }
        "min_rows" => o.min_rows = num("a whole number")?.max(1.0) as usize,
        "max_rows" => o.max_rows = num("a whole number")?.clamp(1.0, 10_000_000.0) as usize,
        "epochs" => o.epochs = num("a whole number from 1 to 1000")?.clamp(1.0, 1000.0) as usize,
        "l2" => o.l2 = num("a number")?.max(0.0) as f32,
        "learning_rate" => o.learning_rate = num("a number")?.clamp(1e-6, 10.0) as f32,
        _ => return Err(err(format!("CREATE CLASSIFIER has no option {key}; the form is {CREATE_FORM}"))),
    }
    Ok(())
}

fn quote_ident(s: &str) -> String {
    match s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.starts_with(|c: char| c.is_ascii_digit()) {
        true => s.to_string(),
        false => format!("\"{}\"", s.replace('"', "\"\"")),
    }
}

impl Create {
    /// The query that reads the rows it trains on.
    pub fn training_sql(&self) -> String {
        let (f, l) = (quote_ident(&self.field), quote_ident(&self.labels));
        let cond = self.condition.as_ref().map_or(String::new(), |c| format!(" AND ({c})"));
        format!("SELECT {f}, {l} FROM {} WHERE {l} IS NOT NULL{cond} LIMIT {}", quote_ident(&self.table), self.options.max_rows)
    }

    /// Checks the statement against the table's schema: the field a VECTOR,
    /// the labels a KEYWORD, TEXT, INT or BOOL column. Gives the model the
    /// field is embedded by, if any.
    pub fn check(&self, schema: &Schema) -> Result<Option<String>> {
        super::function::check_function_name(&self.name)?;
        let f = schema.field(&self.field).ok_or_else(|| err(format!("CREATE CLASSIFIER: {} has no field {}", schema.table, self.field)))?;
        let FieldType::Vector { .. } = f.ty else { return Err(err(format!("CREATE CLASSIFIER: {} is not a VECTOR field; a classifier reads a field of embeddings", f.name))) };
        if f.multi {
            return Err(err(format!("CREATE CLASSIFIER: {} holds several vectors a row; a classifier reads a field of one", f.name)));
        }
        let l = schema.field(&self.labels).ok_or_else(|| err(format!("CREATE CLASSIFIER: {} has no column {}", schema.table, self.labels)))?;
        if matches!(l.ty, FieldType::Vector { .. } | FieldType::Edges | FieldType::Sparse | FieldType::Json) {
            return Err(err(format!("CREATE CLASSIFIER: the labels are the values of a KEYWORD, TEXT, INT or BOOL column, and {} is not one", l.name)));
        }
        Ok(f.embed.as_ref().map(|e| e.model.clone()))
    }

    /// Trains on `rs`, the rows of [`Create::training_sql`].
    pub fn train(&self, schema: &Schema, rs: &ResultSet) -> Result<Trained> {
        let model = self.check(schema)?;
        let t0 = Instant::now();
        let mut xs: Vec<Arc<[f32]>> = Vec::with_capacity(rs.rows.len());
        let mut names: Vec<String> = Vec::with_capacity(rs.rows.len());
        let mut skipped = 0u64;
        for row in &rs.rows {
            let (Some(Value::Vector(v)), Some(l)) = (row.first(), row.get(1)) else {
                skipped += 1;
                continue;
            };
            let label = match l {
                Value::Str(s) if !s.trim().is_empty() => s.clone(),
                Value::Int(i) => i.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::List(ls) if ls.len() == 1 => match &ls[0] {
                    Value::Str(s) => s.clone(),
                    _ => {
                        skipped += 1;
                        continue;
                    }
                },
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            if v.iter().any(|c| !c.is_finite()) || v.is_empty() {
                skipped += 1;
                continue;
            }
            xs.push(v.clone());
            names.push(label);
        }
        let dim = xs.first().map_or(0, |v| v.len());
        if xs.iter().any(|v| v.len() != dim) {
            return Err(err("CREATE CLASSIFIER: the vectors are not all one size"));
        }
        // The labels with enough rows, most rows first.
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for n in &names {
            *counts.entry(n.as_str()).or_default() += 1;
        }
        let mut kept: Vec<(&str, usize)> = counts.into_iter().filter(|&(_, c)| c >= self.options.min_rows).collect();
        kept.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        if kept.len() < 2 {
            return Err(err(format!(
                "CREATE CLASSIFIER: {} has {} label{} with {} rows or more, and a classifier tells two or more apart",
                self.labels,
                kept.len(),
                if kept.len() == 1 { "" } else { "s" },
                self.options.min_rows
            )));
        }
        if kept.len() > MAX_LABELS {
            return Err(err(format!("CREATE CLASSIFIER: {} has {} labels, and a classifier has {MAX_LABELS} at most: raise min_rows", self.labels, kept.len())));
        }
        let labels: Vec<String> = kept.iter().map(|(l, _)| l.to_string()).collect();
        let index: HashMap<&str, usize> = labels.iter().enumerate().map(|(i, l)| (l.as_str(), i)).collect();
        let (mut train, mut test) = (Vec::new(), Vec::new());
        let cut = (self.options.holdout * 1_000_000.0) as u64;
        for (v, n) in xs.iter().zip(&names) {
            let Some(&y) = index.get(n.as_str()) else {
                skipped += 1;
                continue;
            };
            let x = unit(v);
            match holdout_hash(&x) % 1_000_000 < cut {
                true => test.push((x, y)),
                false => train.push((x, y)),
            }
        }
        if train.is_empty() {
            return Err(err("CREATE CLASSIFIER: no rows to train on after the holdout: lower holdout"));
        }
        let (w, bias) = match self.options.method {
            Method::Centroid => centroids(&train, labels.len(), dim),
            Method::Logistic => logistic(&train, labels.len(), dim, &self.options),
        };
        let mut c = Classifier::quantized(&w, bias, labels, dim);
        c.method = self.options.method;
        c.min_score = self.options.min_score;
        c.table = schema.table.clone();
        c.field = self.field.clone();
        c.labels_from = self.labels.clone();
        c.model = model;
        c.trained_rows = train.len() as u64;
        let correct = test.iter().filter(|(x, y)| c.best(x).0 == *y).count();
        let accuracy = (!test.is_empty()).then(|| correct as f64 / test.len() as f64);
        c.holdout_rows = test.len() as u64;
        c.holdout_accuracy = accuracy;
        let train_correct = train.iter().filter(|(x, y)| c.best(x).0 == *y).count();
        Ok(Trained {
            report: Report {
                name: self.name.clone(),
                method: self.options.method,
                rows: (train.len() + test.len()) as u64,
                skipped,
                labels: c.labels.len(),
                train_rows: train.len() as u64,
                train_accuracy: train_correct as f64 / train.len() as f64,
                holdout_rows: test.len() as u64,
                holdout_accuracy: accuracy,
                took_ms: t0.elapsed().as_secs_f64() * 1e3,
            },
            classifier: c,
        })
    }
}

/// A trained classifier and what its training measured.
#[derive(Clone, Debug)]
pub struct Trained {
    pub classifier: Classifier,
    pub report: Report,
}

/// What a training measured.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub name: String,
    pub method: Method,
    pub rows: u64,
    /// Rows without a vector or a label, or with a label of too few rows.
    pub skipped: u64,
    pub labels: usize,
    pub train_rows: u64,
    pub train_accuracy: f64,
    pub holdout_rows: u64,
    pub holdout_accuracy: Option<f64>,
    pub took_ms: f64,
}

impl Trained {
    /// The stored function the classifier is, as `CREATE [OR REPLACE]
    /// FUNCTION`.
    pub fn function_sql(&self, create: &Create) -> String {
        let c = &self.classifier;
        let r = &self.report;
        let mut what = format!("classifier of {}.{} by {}: {}, {} labels, {} rows", c.table, c.field, c.labels_from, r.method.name(), r.labels, r.rows);
        if let Some(a) = r.holdout_accuracy {
            what.push_str(&format!(", {:.1}% right on {} held out", a * 100.0, r.holdout_rows));
        }
        format!(
            "CREATE {}FUNCTION {}(v VECTOR, returns TEXT DEFAULT 'label') RETURNS KEYWORD OPTIONS (description = {}) AS {NAME}(:v, {}, returns => :returns)",
            if create.replace { "OR REPLACE " } else { "" },
            create.name,
            quote(&what),
            quote(&c.encode()),
        )
    }

    /// The report as the answer of the statement.
    pub fn result_set(&self) -> ResultSet {
        let r = &self.report;
        let f = |x: f64| Value::Float((x * 10_000.0).round() / 10_000.0);
        let columns = ["classifier", "method", "labels", "rows", "train_rows", "train_accuracy", "holdout_rows", "holdout_accuracy", "skipped", "took_ms"];
        let row = vec![
            Value::Str(r.name.clone()),
            Value::Str(r.method.name().into()),
            Value::Int(r.labels as i64),
            Value::Int(r.rows as i64),
            Value::Int(r.train_rows as i64),
            f(r.train_accuracy),
            Value::Int(r.holdout_rows as i64),
            r.holdout_accuracy.map_or(Value::Null, f),
            Value::Int(r.skipped as i64),
            Value::Float(r.took_ms.round()),
        ];
        ResultSet { columns: columns.iter().map(|c| c.to_string()).collect(), rows: vec![row], keys: vec![Value::Null], scores: vec![0.0], parts: None, stats: ExecStats::default() }
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn unit(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    match n > 0.0 {
        true => v.iter().map(|x| x / n).collect(),
        false => v.to_vec(),
    }
}

/// The same for the same vector, on every machine.
fn holdout_hash(v: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in v {
        for b in x.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    // splitmix's finish, so the low digits are mixed too.
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

fn centroids(train: &[(Vec<f32>, usize)], labels: usize, dim: usize) -> (Vec<f32>, Vec<f32>) {
    let mut w = vec![0f32; labels * dim];
    for (x, y) in train {
        for (a, b) in w[y * dim..(y + 1) * dim].iter_mut().zip(x) {
            *a += b;
        }
    }
    for row in w.chunks_mut(dim) {
        let n = row.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 0.0 {
            row.iter_mut().for_each(|x| *x *= CENTROID_SCALE / n);
        }
    }
    (w, vec![0.0; labels])
}

/// The steps a logistic training takes at least, however few its rows.
const MIN_STEPS: usize = 300;

/// Softmax regression by Adam on batches, the batch's gradient computed
/// over the cores.
fn logistic(train: &[(Vec<f32>, usize)], labels: usize, dim: usize, o: &Options) -> (Vec<f32>, Vec<f32>) {
    let (n, width) = (train.len(), dim + 1);
    let mut w = vec![0f32; labels * width];
    let (mut m, mut v) = (vec![0f32; w.len()], vec![0f32; w.len()]);
    let (b1, b2, eps) = (0.9f32, 0.999f32, 1e-8f32);
    let batch = 512.min(n).max(1);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
    let mut order: Vec<usize> = (0..n).collect();
    let mut rng: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut t = 0i32;
    // Few rows make few batches an epoch: at least MIN_STEPS steps in all.
    let epochs = o.epochs.max(MIN_STEPS.div_ceil(n.div_ceil(batch)));
    for epoch in 0..epochs {
        // A seeded shuffle, the same every training.
        for i in (1..n).rev() {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            order.swap(i, (rng % (i as u64 + 1)) as usize);
        }
        // The rate falls by half over the epochs, on a cosine.
        let lr = o.learning_rate * (0.55 + 0.45 * (std::f32::consts::PI * epoch as f32 / epochs as f32).cos());
        for chunk in order.chunks(batch) {
            let grad = gradient(train, chunk, &w, labels, dim, threads);
            t += 1;
            let (c1, c2) = (1.0 - b1.powi(t), 1.0 - b2.powi(t));
            let scale = 1.0 / chunk.len() as f32;
            for (k, g) in grad.iter().enumerate() {
                // No penalty on the bias.
                let g = g * scale + if k % width < dim { o.l2 * w[k] } else { 0.0 };
                m[k] = b1 * m[k] + (1.0 - b1) * g;
                v[k] = b2 * v[k] + (1.0 - b2) * g * g;
                w[k] -= lr * (m[k] / c1) / ((v[k] / c2).sqrt() + eps);
            }
        }
    }
    let mut weights = Vec::with_capacity(labels * dim);
    let mut bias = Vec::with_capacity(labels);
    for row in w.chunks(width) {
        weights.extend_from_slice(&row[..dim]);
        bias.push(row[dim]);
    }
    (weights, bias)
}

/// The summed gradient of the softmax loss over the rows `chunk` names,
/// a row of `dim` weights and a bias a label.
fn gradient(train: &[(Vec<f32>, usize)], chunk: &[usize], w: &[f32], labels: usize, dim: usize, threads: usize) -> Vec<f32> {
    let width = dim + 1;
    let part = |rows: &[usize]| -> Vec<f32> {
        let mut g = vec![0f32; labels * width];
        let mut z = vec![0f32; labels];
        for &r in rows {
            let (x, y) = (&train[r].0, train[r].1);
            for (l, zl) in z.iter_mut().enumerate() {
                let row = &w[l * width..(l + 1) * width];
                *zl = dot(&row[..dim], x) + row[dim];
            }
            let max = z.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mut sum = 0.0;
            for zl in z.iter_mut() {
                *zl = (*zl - max).exp();
                sum += *zl;
            }
            for (l, zl) in z.iter().enumerate() {
                let d = zl / sum - if l == y { 1.0 } else { 0.0 };
                if d.abs() < 1e-7 {
                    continue;
                }
                let row = &mut g[l * width..(l + 1) * width];
                for (a, b) in row[..dim].iter_mut().zip(x) {
                    *a += d * b;
                }
                row[dim] += d;
            }
        }
        g
    };
    if threads <= 1 || chunk.len() < 64 {
        return part(chunk);
    }
    let per = chunk.len().div_ceil(threads);
    let parts: Vec<Vec<f32>> = std::thread::scope(|s| {
        let hs: Vec<_> = chunk.chunks(per).map(|c| s.spawn(move || part(c))).collect();
        hs.into_iter().map(|h| h.join().expect("a gradient thread")).collect()
    });
    let mut g = parts[0].clone();
    for p in &parts[1..] {
        for (a, b) in g.iter_mut().zip(p) {
            *a += b;
        }
    }
    g
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Four sums, which the compiler keeps in vector registers.
    let mut s = [0f32; 4];
    let (ca, cb) = (a.chunks_exact(4), b.chunks_exact(4));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for k in 0..4 {
            s[k] += x[k] * y[k];
        }
    }
    let mut t = s[0] + s[1] + s[2] + s[3];
    for (x, y) in ra.iter().zip(rb) {
        t += x * y;
    }
    t
}

/// 8-bit weights as floats, each row times its scale.
fn scaled(q: &[i8], scale: &[f32], dim: usize) -> Vec<f32> {
    q.chunks(dim.max(1)).zip(scale).flat_map(|(row, s)| row.iter().map(move |&w| w as f32 * s)).collect()
}

/// A 128-bit hash of `bytes`, read 8 bytes at a time: a model's text is
/// hundreds of kilobytes, and SHA-256 of it would cost a query a
/// millisecond.
fn fast_hash(bytes: &[u8]) -> [u8; 16] {
    let (mut a, mut b) = (0x243f_6a88_85a3_08d3u64 ^ bytes.len() as u64, 0x1319_8a2e_0370_7344u64);
    let mut words = bytes.chunks_exact(8);
    for w in &mut words {
        let w = u64::from_le_bytes(w.try_into().expect("8 bytes"));
        a = (a ^ w).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29);
        b = (b ^ w.rotate_left(17)).wrapping_mul(0xc2b2_ae3d_27d4_eb4f).rotate_left(31);
    }
    let mut tail = [0u8; 8];
    tail[..words.remainder().len()].copy_from_slice(words.remainder());
    let w = u64::from_le_bytes(tail);
    a = (a ^ w).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    b = (b ^ w).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    let mix = |mut h: u64| {
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^ (h >> 31)
    };
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&mix(a ^ b.rotate_left(7)).to_le_bytes());
    out[8..].copy_from_slice(&mix(b ^ a.rotate_left(13)).to_le_bytes());
    out
}

/// A classifier: a row of 8-bit weights and a bias a label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Classifier {
    /// The format of the model's text.
    pub v: u32,
    pub method: Method,
    pub table: String,
    pub field: String,
    /// The column the labels came from.
    pub labels_from: String,
    /// The embedding model of the field, if it has one.
    #[serde(default)]
    pub model: Option<String>,
    pub dim: usize,
    pub labels: Vec<String>,
    /// Each label's weights are `w[l] * scale[l]`.
    pub scale: Vec<f32>,
    pub bias: Vec<f32>,
    /// The weights, a label's `dim` after another's, as bytes of i8 in
    /// base64.
    pub w: String,
    #[serde(default)]
    pub min_score: f32,
    #[serde(default)]
    pub trained_rows: u64,
    #[serde(default)]
    pub holdout_rows: u64,
    #[serde(default)]
    pub holdout_accuracy: Option<f64>,
    /// The weights as floats, each row times its scale, to score with.
    #[serde(skip)]
    weights: Vec<f32>,
}

impl Classifier {
    fn quantized(w: &[f32], bias: Vec<f32>, labels: Vec<String>, dim: usize) -> Classifier {
        let mut weights = Vec::with_capacity(w.len());
        let mut scale = Vec::with_capacity(labels.len());
        for row in w.chunks(dim.max(1)) {
            let max = row.iter().fold(0f32, |m, x| m.max(x.abs()));
            let s = if max > 0.0 { max / 127.0 } else { 1.0 };
            scale.push(s);
            weights.extend(row.iter().map(|x| (x / s).round().clamp(-127.0, 127.0) as i8));
        }
        let bytes: Vec<u8> = weights.iter().map(|&b| b as u8).collect();
        let weights = scaled(&weights, &scale, dim);
        Classifier {
            v: 1,
            method: Method::Logistic,
            table: String::new(),
            field: String::new(),
            labels_from: String::new(),
            model: None,
            dim,
            labels,
            scale,
            bias,
            w: base64::engine::general_purpose::STANDARD.encode(bytes),
            min_score: 0.0,
            trained_rows: 0,
            holdout_rows: 0,
            holdout_accuracy: None,
            weights,
        }
    }

    /// The model as text, the second argument of [`NAME`].
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("a classifier serializes")
    }

    /// The model of text `text` ([`Classifier::encode`]).
    pub fn decode(text: &str) -> Result<Classifier> {
        let mut c: Classifier = serde_json::from_str(text).map_err(|e| err(format!("{NAME}(): the model is not one CREATE CLASSIFIER made ({e}); {USAGE}")))?;
        if c.v != 1 {
            return Err(err(format!("{NAME}(): the model is of format {}, and this ChloeDB reads format 1", c.v)));
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(c.w.as_bytes()).map_err(|e| err(format!("{NAME}(): the model's weights: {e}")))?;
        if c.labels.len() < 2 || bytes.len() != c.labels.len() * c.dim || c.scale.len() != c.labels.len() || c.bias.len() != c.labels.len() {
            return Err(err(format!("{NAME}(): the model's weights do not fit its labels")));
        }
        let q: Vec<i8> = bytes.into_iter().map(|b| b as i8).collect();
        c.weights = scaled(&q, &c.scale, c.dim);
        Ok(c)
    }

    /// Each label's probability for `x` (of length `dim`, made unit
    /// length here), in the order of [`Classifier::labels`].
    pub fn probabilities(&self, x: &[f32]) -> Vec<f32> {
        let x = unit(x);
        let mut z: Vec<f32> = (0..self.labels.len())
            .map(|l| {
                dot(&self.weights[l * self.dim..(l + 1) * self.dim], &x) + self.bias[l]
            })
            .collect();
        let max = z.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0;
        for v in z.iter_mut() {
            *v = (*v - max).exp();
            sum += *v;
        }
        z.iter_mut().for_each(|v| *v /= sum);
        z
    }

    /// The best label of `x` and its probability.
    pub fn best(&self, x: &[f32]) -> (usize, f32) {
        let p = self.probabilities(x);
        p.iter().enumerate().fold((0, f32::NEG_INFINITY), |b, (i, &v)| if v > b.1 { (i, v) } else { b })
    }

    /// The `k` best labels of `x`, best first, with their probabilities.
    pub fn top(&self, x: &[f32], k: usize) -> Vec<(String, f32)> {
        let p = self.probabilities(x);
        let mut idx: Vec<usize> = (0..p.len()).collect();
        idx.sort_by(|&a, &b| p[b].total_cmp(&p[a]).then(a.cmp(&b)));
        idx.into_iter().take(k).map(|i| (self.labels[i].clone(), p[i])).collect()
    }

    /// What `classify(x, model, returns => r)` gives.
    pub fn value(&self, x: &Value, returns: Returns) -> Result<Value> {
        let v = match x {
            Value::Null => return Ok(Value::Null),
            Value::Vector(v) => v.clone(),
            Value::List(l) if l.is_empty() => return Ok(Value::Null),
            Value::Str(s) => match serde_json::from_str::<Vec<f32>>(s) {
                Ok(v) => v.into(),
                Err(_) => return Err(err(format!("{NAME}(): the first argument is a vector, not text: give the text as a parameter for the server to embed, {{\"embed\": \"some words\", \"field\": \"{}\", \"table\": \"{}\"}}", self.field, self.table))),
            },
            v => return Err(err(format!("{NAME}(): the first argument is a vector, not {}", super::hint::kind_of(v)))),
        };
        if v.len() != self.dim {
            return Err(err(format!("{NAME}(): the model reads vectors of {} numbers, and this one has {}", self.dim, v.len())));
        }
        let p = self.probabilities(&v);
        let (i, best) = p.iter().enumerate().fold((0, f32::NEG_INFINITY), |b, (i, &v)| if v > b.1 { (i, v) } else { b });
        Ok(match returns {
            Returns::Score => Value::Float(best as f64),
            Returns::Label if best < self.min_score => Value::Null,
            Returns::Label => Value::Str(self.labels[i].clone()),
            Returns::Labels => {
                let floor = self.min_score.max(LABELS_FLOOR);
                let mut idx: Vec<usize> = (0..p.len()).filter(|&l| l == i || p[l] >= floor).collect();
                idx.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
                if best < self.min_score {
                    idx.clear();
                }
                Value::List(idx.into_iter().map(|l| Value::Str(self.labels[l].clone())).collect())
            }
        })
    }
}

/// What a call gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Returns {
    Label,
    Score,
    Labels,
}

struct Cache {
    by: HashMap<[u8; 16], Arc<Classifier>>,
    order: VecDeque<[u8; 16]>,
    /// Stored functions' classifiers, by the function (kept alive here, so
    /// its address names it).
    functions: Vec<(Arc<super::function::Function>, Arc<Classifier>)>,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache { by: HashMap::new(), order: VecDeque::new(), functions: Vec::new() }))
}

/// The model of text `text`, decoded once and kept.
pub fn model(text: &str) -> Result<Arc<Classifier>> {
    let key = fast_hash(text.as_bytes());
    if let Some(c) = cache().lock().unwrap_or_else(|e| e.into_inner()).by.get(&key) {
        return Ok(c.clone());
    }
    let c = Arc::new(Classifier::decode(text)?);
    let mut m = cache().lock().unwrap_or_else(|e| e.into_inner());
    if m.by.insert(key, c.clone()).is_none() {
        m.order.push_back(key);
    }
    while m.order.len() > CACHED {
        if let Some(k) = m.order.pop_front() {
            m.by.remove(&k);
        }
    }
    Ok(c)
}

/// The classifier stored function `f` is, if it is one `CREATE
/// CLASSIFIER` made: read once a version.
pub fn of_function(f: &Arc<super::function::Function>) -> Result<Option<Arc<Classifier>>> {
    if let Some((_, c)) = cache().lock().unwrap_or_else(|e| e.into_inner()).functions.iter().find(|(g, _)| Arc::ptr_eq(g, f)) {
        return Ok(Some(c.clone()));
    }
    let Some(c) = of_body(&f.def.body)? else { return Ok(None) };
    let mut m = cache().lock().unwrap_or_else(|e| e.into_inner());
    m.functions.push((f.clone(), c.clone()));
    if m.functions.len() > CACHED {
        m.functions.remove(0);
    }
    Ok(Some(c))
}

/// The classifier a stored function's body calls, if it is one
/// `CREATE CLASSIFIER` made.
pub fn of_body(body: &str) -> Result<Option<Arc<Classifier>>> {
    if !body.trim_start().get(..NAME.len()).is_some_and(|h| h.eq_ignore_ascii_case(NAME)) {
        return Ok(None);
    }
    let toks = Tokenizer::new(&GenericDialect {}, body).tokenize().map_err(|e| err(format!("{NAME}(): {e}")))?;
    match toks.into_iter().find_map(|t| if let Token::SingleQuotedString(s) = t { Some(s) } else { None }) {
        Some(text) => model(&text).map(Some),
        None => Ok(None),
    }
}

/// A text's classes ([`crate::Db::classify`]).
#[derive(Clone, Debug, Serialize)]
pub struct Classified {
    pub classifier: String,
    pub version: i64,
    pub table: String,
    pub field: String,
    pub labels_from: String,
    /// The best label, `None` below the model's `min_score`.
    pub label: Option<String>,
    pub score: f32,
    pub min_score: f32,
    /// The best labels, best first, with their probabilities.
    pub top: Vec<(String, f32)>,
}

impl Classified {
    pub fn of(name: &str, version: i64, c: &Classifier, x: &[f32], k: usize) -> Result<Classified> {
        if x.len() != c.dim {
            return Err(err(format!("{NAME}(): the model reads vectors of {} numbers, and this one has {}", c.dim, x.len())));
        }
        let top = c.top(x, k.max(1));
        let (best, score) = top.first().cloned().unwrap_or_default();
        Ok(Classified {
            classifier: name.to_string(),
            version,
            table: c.table.clone(),
            field: c.field.clone(),
            labels_from: c.labels_from.clone(),
            label: (score >= c.min_score).then_some(best),
            score,
            min_score: c.min_score,
            top,
        })
    }
}

/// Whether `f` calls [`NAME`].
pub fn is_call(f: &sql::Function) -> bool {
    f.name.to_string().eq_ignore_ascii_case(NAME)
}

/// A call of [`NAME`]: its vector, its model, and what it returns.
pub fn parse_call(f: &sql::Function, params: &Params) -> Result<(Expr, Arc<Classifier>, Returns)> {
    if f.over.is_some() || f.filter.is_some() || !f.within_group.is_empty() {
        return Err(err(format!("{f}: {USAGE}")));
    }
    let FunctionArguments::List(l) = &f.args else { return Err(err(USAGE)) };
    let mut positional: Vec<&Expr> = Vec::new();
    let mut returns = Returns::Label;
    for a in &l.args {
        let (name, e) = match a {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                positional.push(e);
                continue;
            }
            FunctionArg::Named { name, arg: FunctionArgExpr::Expr(e), .. } => (name.value.to_ascii_lowercase(), e),
            FunctionArg::ExprNamed { name: Expr::Identifier(name), arg: FunctionArgExpr::Expr(e), .. } => (name.value.to_ascii_lowercase(), e),
            _ => return Err(err(USAGE)),
        };
        if name != "returns" {
            return Err(err(format!("{f}: {NAME}() has no option {name}; {USAGE}")));
        }
        // A stored function's own parameter, unbound when its body is checked.
        if let Expr::Value(v) = e {
            if let sql::Value::Placeholder(p) = &v.value {
                if !params.0.contains_key(p.trim_start_matches([':', '$', '@', '?'])) {
                    continue;
                }
            }
        }
        returns = match super::compound::value_of(e, params)? {
            Value::Str(s) if s.eq_ignore_ascii_case("label") => Returns::Label,
            Value::Str(s) if s.eq_ignore_ascii_case("score") => Returns::Score,
            Value::Str(s) if s.eq_ignore_ascii_case("labels") => Returns::Labels,
            v => return Err(err(format!("{NAME}(): returns is 'label', 'score' or 'labels', not {}", super::hint::sql_value(&v)))),
        };
    }
    let [x, m] = positional.as_slice() else { return Err(err(format!("{f}: {USAGE}"))) };
    let Value::Str(text) = super::compound::value_of(m, params)? else { return Err(err(format!("{f}: the model is text; {USAGE}"))) };
    Ok(((*x).clone(), model(&text)?, returns))
}

/// Whether `e` is a value of no row: a literal or a parameter.
fn constant(e: &Expr) -> bool {
    match e {
        Expr::Value(_) => true,
        Expr::UnaryOp { expr, .. } | Expr::Nested(expr) | Expr::Cast { expr, .. } => constant(expr),
        Expr::BinaryOp { left, right, .. } => constant(left) && constant(right),
        Expr::Array(a) => a.elem.iter().all(constant),
        _ => false,
    }
}

struct Fold<'p> {
    params: &'p Params,
    folded: usize,
    failed: Option<Error>,
}

impl VisitorMut for Fold<'_> {
    type Break = ();

    fn post_visit_expr(&mut self, e: &mut Expr) -> std::ops::ControlFlow<()> {
        let Expr::Function(f) = e else { return std::ops::ControlFlow::Continue(()) };
        if !is_call(f) {
            return std::ops::ControlFlow::Continue(());
        }
        let r = (|| -> Result<Option<Value>> {
            let (x, c, returns) = parse_call(f, self.params)?;
            // A list is no literal of a row expression: the labels are
            // computed with the rows, as a column.
            if !constant(&x) || returns == Returns::Labels {
                return Ok(None);
            }
            let v = super::compound::value_of(&x, self.params)?;
            c.value(&v, returns).map(Some)
        })();
        match r {
            Ok(Some(v)) => {
                *e = literal(&v);
                self.folded += 1;
            }
            Ok(None) => {}
            Err(x) => {
                self.failed = Some(x);
                return std::ops::ControlFlow::Break(());
            }
        }
        std::ops::ControlFlow::Continue(())
    }
}

fn literal(v: &Value) -> Expr {
    let s = |t: &str| Expr::Value(sql::Value::SingleQuotedString(t.to_string()).into());
    match v {
        Value::Null => Expr::Value(sql::Value::Null.into()),
        Value::Str(t) => s(t),
        Value::Float(x) => Expr::Value(sql::Value::Number(format!("{x:?}"), false).into()),
        Value::List(xs) => Expr::Array(sql::Array { elem: xs.iter().map(literal).collect(), named: true }),
        v => s(&super::hint::sql_value(v)),
    }
}

/// Computes each call of [`NAME`] in `q` whose vector is a value of no
/// row (a parameter, a literal), the call replaced by its value; how many
/// it computed.
pub fn fold(q: &mut sql::Query, params: &Params) -> Result<usize> {
    let mut f = Fold { params, folded: 0, failed: None };
    let _ = q.visit(&mut f);
    match f.failed {
        Some(e) => Err(e),
        None => Ok(f.folded),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<(Vec<f32>, usize)> {
        // Three labels, each near an axis.
        (0..n)
            .map(|i| {
                let y = i % 3;
                let mut x = vec![0.1f32; 8];
                x[y] = 1.0;
                x[3 + (i % 5)] += 0.3;
                (unit(&x), y)
            })
            .collect()
    }

    #[test]
    fn statements_parse() {
        let Some(Statement::Create(c)) = parse_statement("CREATE OR REPLACE CLASSIFIER Topic ON products(emb) LABELS category OPTIONS (method = 'centroid', holdout = 0.1) WHERE price > 10 AND brand <> 'x';").unwrap() else { panic!() };
        assert_eq!((c.name.as_str(), c.table.as_str(), c.field.as_str(), c.labels.as_str(), c.replace), ("topic", "products", "emb", "category", true));
        assert_eq!(c.options.method, Method::Centroid);
        assert_eq!(c.condition.as_deref(), Some("price > 10 AND brand <> 'x'"));
        assert_eq!(c.training_sql(), "SELECT emb, category FROM products WHERE category IS NOT NULL AND (price > 10 AND brand <> 'x') LIMIT 200000");
        assert_eq!(parse_statement("DROP CLASSIFIER IF EXISTS topic").unwrap(), Some(Statement::Drop { name: "topic".into(), if_exists: true }));
        assert_eq!(parse_statement("SELECT 'CLASSIFIER' FROM t").unwrap(), None);
        assert!(parse_statement("CREATE CLASSIFIER t ON p(emb) LABELS c OPTIONS (speed = 2)").unwrap_err().to_string().contains("no option speed"));
    }

    #[test]
    fn both_methods_learn_and_survive_their_text() {
        let train = rows(300);
        for method in [Method::Centroid, Method::Logistic] {
            let (w, b) = match method {
                Method::Centroid => centroids(&train, 3, 8),
                Method::Logistic => logistic(&train, 3, 8, &Options { epochs: 50, learning_rate: 0.05, ..Options::default() }),
            };
            let c = Classifier::quantized(&w, b, vec!["a".into(), "b".into(), "c".into()], 8);
            let right = train.iter().filter(|(x, y)| c.best(x).0 == *y).count();
            assert_eq!(right, train.len(), "{method:?}");
            let back = Classifier::decode(&c.encode()).unwrap();
            assert_eq!(back.best(&train[1].0), c.best(&train[1].0));
            let x = Value::Vector(train[2].0.clone().into());
            assert_eq!(back.value(&x, Returns::Label).unwrap(), Value::Str("c".into()));
            assert!(matches!(back.value(&x, Returns::Score).unwrap(), Value::Float(p) if p > 0.34));
        }
    }
}
