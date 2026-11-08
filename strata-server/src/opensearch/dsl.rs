//! OpenSearch's Query DSL as Strata SQL: a query becomes a condition (the
//! `WHERE`) and a score (the `ORDER BY`), which `bm25`, `similarity` and
//! arithmetic over them make.
//!
//! Full-text queries (`match`, `match_phrase`, `multi_match`,
//! `query_string`...) on `text` fields become `MATCH` on their columns
//! and `bm25` scores; term-level queries (`term`, `terms`, `range`,
//! `exists`, `prefix`, `ids`) become comparisons on `_source` paths, the
//! key, or a date's column; `bool`, `dis_max`, `constant_score`,
//! `function_score` and `knn` combine them.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value as Json};
use strata::text::analyzer::{normalize_word, Analyzer};

use super::mapping::{ident, lit, parse_date, Index, Kind, ID, SOURCE};
use super::OsError;

/// A query's condition (`None`: every row) and score.
#[derive(Clone, Debug, Default)]
pub struct Compiled {
    pub pred: Option<String>,
    pub score: Score,
}

/// A query's score as SQL: none (a filter), a constant, or an expression
/// of `bm25` and `similarity` leaves.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Score {
    #[default]
    None,
    Const(f64),
    Expr(String),
}

impl Score {
    fn add(self, other: Score) -> Score {
        match (self, other) {
            (Score::None, s) | (s, Score::None) => s,
            (Score::Const(a), Score::Const(b)) => Score::Const(a + b),
            (Score::Expr(e), Score::Const(c)) | (Score::Const(c), Score::Expr(e)) => Score::Expr(format!("{e} + {}", num(c))),
            (Score::Expr(a), Score::Expr(b)) => Score::Expr(format!("{a} + {b}")),
        }
    }

    fn scale(self, by: f64) -> Score {
        if by == 1.0 {
            return self;
        }
        match self {
            Score::None => Score::None,
            Score::Const(c) => Score::Const(c * by),
            Score::Expr(e) => Score::Expr(format!("{} * ({e})", num(by))),
        }
    }

    /// The best of several scores, plus `tie` times the others
    /// (`dis_max`, `best_fields`).
    fn best(scores: Vec<Score>, tie: f64) -> Score {
        let exprs: Vec<String> = scores.iter().filter_map(|s| match s {
            Score::Expr(e) => Some(e.clone()),
            _ => None,
        }).collect();
        let consts: Vec<f64> = scores.iter().filter_map(|s| if let Score::Const(c) = s { Some(*c) } else { None }).collect();
        match (exprs.len(), consts.is_empty()) {
            (0, true) => Score::None,
            (0, false) => Score::Const(consts.iter().cloned().fold(f64::MIN, f64::max)),
            (1, true) => Score::Expr(exprs[0].clone()),
            _ => {
                let mut all = exprs.clone();
                all.extend(consts.iter().map(|c| num(*c)));
                let best = format!("greatest({})", all.join(", "));
                if tie > 0.0 && exprs.len() > 1 {
                    Score::Expr(format!("{best} + {} * ({} - {best})", num(tie), exprs.join(" + ")))
                } else {
                    Score::Expr(best)
                }
            }
        }
    }

    pub fn is_ranked(&self) -> bool {
        matches!(self, Score::Expr(_))
    }
}

/// A number as SQL writes it.
pub fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

/// The condition no row meets: no row's key is empty, which writes refuse.
pub const NONE: &str = "\"_id\" = ''";

fn and(parts: Vec<String>) -> Option<String> {
    match parts.len() {
        0 => None,
        1 => parts.into_iter().next(),
        _ => Some(parts.iter().map(|p| format!("({p})")).collect::<Vec<_>>().join(" AND ")),
    }
}

fn or(parts: Vec<String>) -> String {
    match parts.len() {
        0 => NONE.to_string(),
        1 => parts.into_iter().next().unwrap_or_default(),
        _ => parts.iter().map(|p| format!("({p})")).collect::<Vec<_>>().join(" OR "),
    }
}

/// Where a field's values are, for a query.
#[derive(Clone, Debug)]
pub enum Target {
    Id,
    /// A TEXT column and its analyzer.
    Text(String, Analyzer),
    /// A path of `_source`, and the kind its mapping gives it (if mapped).
    Json(String, Option<Kind>),
    /// A TIMESTAMP column.
    Date(String),
    /// A VECTOR column.
    Vector(String),
}

impl Target {
    /// The SQL that reads it (in a condition, a sort or a group).
    pub fn sql(&self) -> String {
        match self {
            Target::Id => ident(ID),
            Target::Text(c, _) | Target::Date(c) | Target::Vector(c) => ident(c),
            Target::Json(p, _) => json_path(p),
        }
    }
}

/// `"_source"['a']['b']` for path `a.b`.
pub fn json_path(p: &str) -> String {
    let mut s = ident(SOURCE);
    for k in p.split('.') {
        s.push('[');
        s.push_str(&lit(k));
        s.push(']');
    }
    s
}

/// What a query is compiled against: the index, the words each text
/// field's queries look for (for highlights), and the parameters (query
/// vectors).
pub struct Ctx<'a> {
    pub ix: &'a Index,
    pub words: BTreeMap<String, Vec<String>>,
    pub params: Vec<(String, strata::value::Value)>,
    /// Ranked leaves score every row the condition keeps, not the best
    /// rows of each (`exact => 1`): for a page deep in the results.
    pub exact: bool,
    /// From a `knn` query: the rows it asks for.
    pub knn_k: Option<usize>,
    /// The rows each `bm25` of the score ranks, as `MATCH` conditions: a
    /// ranked query returns only rows that one of them holds, so that a
    /// search adds the others after them ([`Ctx::unscored`]).
    pub scored: Vec<String>,
}

impl<'a> Ctx<'a> {
    pub fn new(ix: &'a Index) -> Ctx<'a> {
        Ctx { ix, words: BTreeMap::new(), params: Vec::new(), exact: false, knn_k: None, scored: Vec::new() }
    }

    /// Where field `name` is.
    pub fn target(&self, name: &str) -> Target {
        if name == ID {
            return Target::Id;
        }
        match self.ix.fields.get(name) {
            Some(f) => match &f.kind {
                Kind::Text { analyzer } => Target::Text(name.to_string(), if *analyzer == "english" { Analyzer::English } else { Analyzer::Simple }),
                Kind::Date => Target::Date(name.to_string()),
                Kind::Vector { .. } => Target::Vector(name.to_string()),
                k => Target::Json(f.source.clone(), Some(k.clone())),
            },
            None => match name.strip_suffix(".keyword") {
                // A text field's keyword sub-field that dynamic mapping
                // would have made: its string in `_source`.
                Some(base) if self.ix.fields.contains_key(base) => Target::Json(base.to_string(), Some(Kind::Keyword)),
                _ => Target::Json(name.to_string(), None),
            },
        }
    }

    /// The text fields a query over `fields` reads (`["title^2",
    /// "body"]`, patterns with `*`; none: every text field), with their
    /// boosts.
    fn text_fields(&self, fields: Option<&Json>) -> Result<Vec<(String, f64)>, OsError> {
        let given: Vec<String> = match fields {
            Some(Json::Array(a)) => a.iter().filter_map(|f| f.as_str().map(String::from)).collect(),
            Some(Json::String(s)) => vec![s.clone()],
            _ => vec!["*".into()],
        };
        let mut out = Vec::new();
        for g in given {
            let (name, boost) = match g.split_once('^') {
                Some((n, b)) => (n.to_string(), b.parse::<f64>().map_err(|_| OsError::parsing(format!("bad boost in field [{g}]")))?),
                None => (g.clone(), 1.0),
            };
            if name.contains('*') {
                for (f, spec) in &self.ix.fields {
                    if glob(&name, f) && matches!(spec.kind, Kind::Text { .. }) {
                        out.push((f.clone(), boost));
                    }
                }
            } else {
                out.push((name, boost));
            }
        }
        Ok(out)
    }

    fn param(&mut self, v: strata::value::Value) -> String {
        let name = format!("v{}", self.params.len());
        self.params.push((name.clone(), v));
        format!(":{name}")
    }

    /// The rows the score's `bm25`s rank none of, as a condition (`None`
    /// when it ranks by none).
    pub fn unscored(&self) -> Option<String> {
        (!self.scored.is_empty()).then(|| format!("NOT ({})", or(self.scored.clone())))
    }

    fn bm25(&mut self, column: &str, words: &[String], fuzzy: bool) -> String {
        let tilde = if fuzzy { "~" } else { "" };
        self.scored.push(format!("MATCH({}, {})", ident(column), lit(&words.iter().map(|w| format!("{w}{tilde}")).collect::<Vec<_>>().join(" OR "))));
        let mut opts = String::from(", syntax => 'plain'");
        if fuzzy {
            opts.push_str(", fuzzy => 'auto'");
        }
        if self.exact {
            opts.push_str(", exact => 1");
        }
        format!("bm25({}, {}{opts})", ident(column), lit(&words.join(" ")))
    }

    fn note_words(&mut self, field: &str, words: &[String]) {
        self.words.entry(field.to_string()).or_default().extend(words.iter().cloned());
    }
}

/// Whether `name` matches pattern `pat`, whose `*` matches any run.
pub fn glob(pat: &str, name: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == name;
    }
    let mut rest = name;
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(p) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(p);
        } else {
            match rest.find(p) {
                Some(at) => rest = &rest[at + p.len()..],
                None => return false,
            }
        }
    }
    true
}

/// The words of a query's text, lowercased and folded, as MATCH reads a
/// word: runs of letters and digits.
pub fn words_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || ('\u{300}'..='\u{36f}').contains(&c) {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(normalize_word(&std::mem::take(&mut cur)));
        }
    }
    if !cur.is_empty() {
        out.push(normalize_word(&cur));
    }
    out.retain(|w| !w.is_empty());
    out
}

/// The words of `words` the analyzer keeps (a stopword it drops matches
/// nothing in MATCH).
fn kept(words: &[String], a: Analyzer) -> Vec<String> {
    words.iter().filter(|w| a.term_of(w).is_some()).cloned().collect()
}

/// A JSON value as a SQL literal of its type.
fn value_sql(v: &Json) -> Result<String, OsError> {
    Ok(match v {
        Json::String(s) => lit(s),
        Json::Number(n) => n.to_string(),
        Json::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        other => return Err(OsError::parsing(format!("[term] query does not support [{other}] as a value"))),
    })
}

/// A value as the field's mapping reads it: a number as text for a
/// keyword, text as a number for a number, `"true"` as a boolean.
fn coerce(v: &Json, kind: Option<&Kind>) -> Json {
    match (kind, v) {
        (Some(Kind::Keyword), Json::Number(n)) => json!(n.to_string()),
        (Some(Kind::Keyword), Json::Bool(b)) => json!(b.to_string()),
        (Some(Kind::Long | Kind::Double), Json::String(s)) => s.trim().parse::<i64>().map(|i| json!(i)).or_else(|_| s.trim().parse::<f64>().map(|f| json!(f))).unwrap_or_else(|_| v.clone()),
        (Some(Kind::Bool), Json::String(s)) if s == "true" || s == "false" => json!(s == "true"),
        _ => v.clone(),
    }
}

/// The boost of a query's body.
fn boost(b: &Json) -> f64 {
    b.get("boost").and_then(Json::as_f64).unwrap_or(1.0)
}

/// `{"field": value}` or `{"field": {"value"/"query": value, ...}}`: the
/// field, its value and its options.
fn field_and(body: &Json, what: &str, key: &str) -> Result<(String, Json, Json), OsError> {
    let m = body.as_object().ok_or_else(|| OsError::parsing(format!("[{what}] query malformed, no start_object after query name")))?;
    let (field, v) = m.iter().find(|(k, _)| !matches!(k.as_str(), "boost" | "_name")).ok_or_else(|| OsError::parsing(format!("[{what}] query has no field")))?;
    match v {
        Json::Object(o) if o.contains_key(key) => Ok((field.clone(), o[key].clone(), v.clone())),
        Json::Object(_) if what == "range" => Ok((field.clone(), Json::Null, v.clone())),
        Json::Object(_) => Err(OsError::parsing(format!("[{what}] query on [{field}] has no [{key}]"))),
        v => Ok((field.clone(), v.clone(), json!({}))),
    }
}

/// How many of `n` clauses `minimum_should_match` asks for: a count
/// (negative: all but that many), a percentage, or `"3<90%"` (all up to
/// 3 clauses, then the percentage).
pub fn min_should(m: &Json, n: usize) -> Result<usize, OsError> {
    let n_i = n as i64;
    let of = |s: &str| -> Result<i64, OsError> {
        let s = s.trim();
        if let Some(p) = s.strip_suffix('%') {
            let p: f64 = p.trim().parse().map_err(|_| OsError::parsing(format!("bad minimum_should_match [{s}]")))?;
            let k = (n as f64 * p.abs() / 100.0).floor() as i64;
            Ok(if p < 0.0 { n_i - k } else { k })
        } else {
            let k: i64 = s.parse().map_err(|_| OsError::parsing(format!("bad minimum_should_match [{s}]")))?;
            Ok(if k < 0 { n_i + k } else { k })
        }
    };
    let k = match m {
        Json::Number(x) => {
            let k = x.as_i64().unwrap_or(1);
            if k < 0 { n_i + k } else { k }
        }
        Json::String(s) => match s.split_once('<') {
            Some((upto, rest)) => {
                let upto: i64 = upto.trim().parse().map_err(|_| OsError::parsing(format!("bad minimum_should_match [{s}]")))?;
                if n_i <= upto { n_i } else { of(rest)? }
            }
            None => of(s)?,
        },
        _ => 1,
    };
    Ok(k.clamp(0, n_i) as usize)
}

/// Compiles a query (`{"match": {...}}`).
pub fn compile(q: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let m = q.as_object().ok_or_else(|| OsError::parsing("a query is an object: {\"match\": {...}}"))?;
    if m.is_empty() {
        return Ok(Compiled { pred: None, score: Score::Const(1.0) });
    }
    if m.len() > 1 {
        return Err(OsError::parsing(format!("[{}] malformed query, expected [END_OBJECT] but found [FIELD_NAME]", m.keys().next().cloned().unwrap_or_default())));
    }
    let (kind, body) = m.iter().next().expect("one query");
    match kind.as_str() {
        "match_all" => Ok(Compiled { pred: None, score: Score::Const(boost(body)) }),
        "match_none" => Ok(Compiled { pred: Some(NONE.into()), score: Score::None }),
        "bool" => compile_bool(body, cx),
        "constant_score" => {
            let f = body.get("filter").ok_or_else(|| OsError::parsing("[constant_score] requires a 'filter' element"))?;
            let c = unscored(cx, |cx| compile(f, cx))?;
            Ok(Compiled { pred: c.pred, score: Score::Const(boost(body)) })
        }
        "dis_max" => {
            let qs = body.get("queries").and_then(Json::as_array).ok_or_else(|| OsError::parsing("[dis_max] requires 'queries' field"))?;
            let tie = body.get("tie_breaker").and_then(Json::as_f64).unwrap_or(0.0);
            let mut preds = Vec::new();
            let mut scores = Vec::new();
            let mut all = false;
            for q in qs {
                let c = compile(q, cx)?;
                match c.pred {
                    Some(p) => preds.push(p),
                    None => all = true,
                }
                scores.push(c.score);
            }
            Ok(Compiled { pred: if all { None } else { Some(or(preds)) }, score: Score::best(scores, tie).scale(boost(body)) })
        }
        "boosting" => {
            let p = body.get("positive").ok_or_else(|| OsError::parsing("[boosting] query requires 'positive' query"))?;
            compile(p, cx)
        }
        "function_score" => compile_function_score(body, cx),
        "nested" => {
            // Objects of an array are not kept apart: the inner query
            // reads the paths of every one.
            let inner = body.get("query").ok_or_else(|| OsError::parsing("[nested] requires 'query' field"))?;
            compile(inner, cx)
        }
        "term" => compile_term(body, cx),
        "terms" => compile_terms(body, cx),
        "ids" => {
            let ids = body.get("values").and_then(Json::as_array).cloned().unwrap_or_default();
            if ids.is_empty() {
                return Ok(Compiled { pred: Some(NONE.into()), score: Score::None });
            }
            let list: Vec<String> = ids.iter().map(|v| lit(&id_text(v))).collect();
            Ok(Compiled { pred: Some(format!("{} IN ({})", ident(ID), list.join(", "))), score: Score::Const(boost(body)) })
        }
        "range" => compile_range(body, cx),
        "exists" => {
            let f = body.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[exists] must be provided with a [field]"))?;
            Ok(Compiled { pred: Some(format!("{} IS NOT NULL", cx.target(f).sql())), score: Score::Const(boost(body)) })
        }
        "prefix" => {
            let (f, v, opts) = field_and(body, "prefix", "value")?;
            let v = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
            let pred = match cx.target(&f) {
                Target::Text(c, _) => {
                    let w = normalize_word(&v);
                    if w.is_empty() || !w.chars().all(char::is_alphanumeric) {
                        return Err(OsError::parsing(format!("Strata's prefix query on text field [{f}] takes one word, not [{v}]")));
                    }
                    format!("MATCH({}, {})", ident(&c), lit(&format!("{w}*")))
                }
                t => format!("PREFIX({}, {})", t.sql(), lit(&v)),
            };
            Ok(Compiled { pred: Some(pred), score: Score::Const(boost(&opts)) })
        }
        "wildcard" => {
            let (f, v, opts) = field_and(body, "wildcard", "value")?;
            let v = v.as_str().map(String::from).or_else(|| opts.get("wildcard").and_then(Json::as_str).map(String::from)).unwrap_or_default();
            let star = v.find(['*', '?']);
            match star {
                Some(at) if at == v.len() - 1 && v.ends_with('*') => compile(&json!({"prefix": {f: {"value": &v[..at], "boost": boost(&opts)}}}), cx),
                None => compile(&json!({"term": {f: {"value": v, "boost": boost(&opts)}}}), cx),
                _ => match cx.target(&f) {
                    t @ (Target::Id | Target::Json(_, Some(Kind::Keyword))) if matches!(t, Target::Id) => {
                        let like = v.replace('%', "\\%").replace('_', "\\_").replace('*', "%").replace('?', "_");
                        Ok(Compiled { pred: Some(format!("{} LIKE {}", t.sql(), lit(&like))), score: Score::Const(boost(&opts)) })
                    }
                    _ => Err(OsError::parsing(format!("Strata's wildcard query on [{f}] takes a trailing * alone, not [{v}]"))),
                },
            }
        }
        "fuzzy" => {
            let (f, v, opts) = field_and(body, "fuzzy", "value")?;
            let v = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
            let edits = match opts.get("fuzziness") {
                Some(Json::Number(n)) => n.as_u64().unwrap_or(2).min(2).to_string(),
                Some(Json::String(s)) if s != "AUTO" && s != "auto" => s.parse::<u64>().unwrap_or(2).min(2).to_string(),
                _ => String::new(),
            };
            match cx.target(&f) {
                Target::Text(c, _) => {
                    let ws = words_of(&v);
                    let Some(w) = ws.first() else { return Ok(Compiled { pred: Some(NONE.into()), score: Score::None }) };
                    cx.note_words(&f, &ws);
                    let score = Score::Expr(cx.bm25(&c, &ws, true)).scale(boost(&opts));
                    Ok(Compiled { pred: Some(format!("MATCH({}, {})", ident(&c), lit(&format!("{w}~{edits}")))), score })
                }
                _ => compile(&json!({"term": {f: v}}), cx),
            }
        }
        "match" => compile_match(body, cx, MatchKind::Words),
        "match_phrase" => compile_match(body, cx, MatchKind::Phrase),
        "match_phrase_prefix" => compile_match(body, cx, MatchKind::PhrasePrefix),
        "match_bool_prefix" => compile_match(body, cx, MatchKind::BoolPrefix),
        "multi_match" => compile_multi_match(body, cx),
        "query_string" | "simple_query_string" => compile_query_string(body, cx, kind == "simple_query_string"),
        "knn" => compile_knn(body, cx),
        other => Err(OsError::parsing(format!("unknown query [{other}]: Strata's OpenSearch API answers bool, match, match_phrase, match_phrase_prefix, multi_match, query_string, simple_query_string, term, terms, ids, range, exists, prefix, wildcard, fuzzy, constant_score, dis_max, function_score, nested, knn, match_all and match_none")).kind("parsing_exception")),
    }
}

/// An id as text: a string, or a number's digits.
pub fn id_text(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        v => v.to_string(),
    }
}

fn clauses(v: Option<&Json>) -> Vec<Json> {
    match v {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Array(a)) => a.clone(),
        Some(v) => vec![v.clone()],
    }
}

/// `f`'s query compiled for its condition alone: its scores rank nothing.
pub fn unscored(cx: &mut Ctx, f: impl FnOnce(&mut Ctx) -> Result<Compiled, OsError>) -> Result<Compiled, OsError> {
    let n = cx.scored.len();
    let c = f(cx);
    cx.scored.truncate(n);
    c
}

fn compile_bool(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let (must, filter, should, must_not) = (clauses(body.get("must")), clauses(body.get("filter")), clauses(body.get("should")), clauses(body.get("must_not")));
    let mut preds = Vec::new();
    let mut score = Score::None;
    for q in &must {
        let c = compile(q, cx)?;
        preds.extend(c.pred);
        score = score.add(c.score);
    }
    for q in &filter {
        preds.extend(unscored(cx, |cx| compile(q, cx))?.pred);
    }
    for q in &must_not {
        // Words a row must not hold are not highlighted.
        let saved = cx.words.clone();
        let c = unscored(cx, |cx| compile(q, cx))?;
        cx.words = saved;
        match c.pred {
            Some(p) => preds.push(format!("NOT ({p})")),
            None => preds.push(NONE.into()),
        }
    }
    let mut should_preds = Vec::new();
    let mut should_all = false;
    for q in &should {
        let c = compile(q, cx)?;
        match c.pred {
            Some(p) => should_preds.push(p),
            None => should_all = true,
        }
        score = score.add(c.score);
    }
    if !should.is_empty() {
        let default = if must.is_empty() && filter.is_empty() { 1 } else { 0 };
        let need = match body.get("minimum_should_match") {
            Some(m) => min_should(m, should.len())?,
            None => default,
        };
        if need > 0 && !should_all {
            if need == 1 {
                preds.push(or(should_preds));
            } else if need >= should_preds.len() {
                preds.extend(should_preds);
            } else {
                preds.push(format!("AT_LEAST({need}, {})", should_preds.iter().map(|p| format!("({p})")).collect::<Vec<_>>().join(", ")));
            }
        }
    }
    // A bool of filters alone scores as match_all does.
    if score == Score::None && filter.is_empty() && must_not.is_empty() && must.is_empty() && should.is_empty() {
        score = Score::Const(1.0);
    }
    if score == Score::None && (!filter.is_empty() || !must_not.is_empty()) && must.is_empty() && should.is_empty() {
        score = Score::Const(0.0);
    }
    Ok(Compiled { pred: and(preds), score: score.scale(boost(body)) })
}

fn compile_term(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let (f, v, opts) = field_and(body, "term", "value")?;
    let b = boost(&opts) * boost(body);
    let insensitive = opts.get("case_insensitive").and_then(Json::as_bool) == Some(true);
    let pred = match cx.target(&f) {
        Target::Id => format!("{} = {}", ident(ID), lit(&id_text(&v))),
        Target::Text(c, a) => {
            // A term on a text field matches a token: one word.
            let ws = kept(&words_of(&id_text(&v)), a);
            match ws.as_slice() {
                [w] => {
                    cx.note_words(&f, &ws);
                    let score = Score::Expr(cx.bm25(&c, &ws, false)).scale(b);
                    return Ok(Compiled { pred: Some(format!("MATCH({}, {})", ident(&c), lit(w))), score });
                }
                _ => NONE.to_string(),
            }
        }
        Target::Date(c) => {
            let t = parse_date(&v).map_err(|why| OsError::parsing(format!("failed to parse date field [{v}] of [{f}]: {why}")))?;
            format!("{} = {}", ident(&c), lit(&strata::timestamp::format(t)))
        }
        Target::Vector(_) => return Err(OsError::parsing(format!("[term] does not take the vector field [{f}]: use [knn]"))),
        Target::Json(p, k) => {
            let v = coerce(&v, k.as_ref());
            if insensitive {
                if let Json::String(s) = &v {
                    let mut cases = vec![s.clone(), s.to_lowercase(), s.to_uppercase()];
                    cases.dedup();
                    let list: Vec<String> = cases.iter().map(|c| lit(c)).collect();
                    format!("{} IN ({})", json_path(&p), list.join(", "))
                } else {
                    format!("{} = {}", json_path(&p), value_sql(&v)?)
                }
            } else {
                format!("{} = {}", json_path(&p), value_sql(&v)?)
            }
        }
    };
    Ok(Compiled { pred: Some(pred), score: Score::Const(b) })
}

fn compile_terms(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let m = body.as_object().ok_or_else(|| OsError::parsing("[terms] query malformed"))?;
    let (f, v) = m.iter().find(|(k, _)| !matches!(k.as_str(), "boost" | "_name")).ok_or_else(|| OsError::parsing("[terms] query requires a field"))?;
    let vals = match v {
        Json::Array(a) => a.clone(),
        Json::Object(_) => return Err(OsError::parsing("[terms] lookup is not supported: list the values")),
        v => vec![v.clone()],
    };
    if vals.is_empty() {
        return Ok(Compiled { pred: Some(NONE.into()), score: Score::None });
    }
    let pred = match cx.target(f) {
        Target::Id => format!("{} IN ({})", ident(ID), vals.iter().map(|v| lit(&id_text(v))).collect::<Vec<_>>().join(", ")),
        Target::Text(c, a) => {
            let ws: Vec<String> = vals.iter().flat_map(|v| kept(&words_of(&id_text(v)), a)).collect();
            if ws.is_empty() {
                NONE.to_string()
            } else {
                cx.note_words(f, &ws);
                format!("MATCH({}, {})", ident(&c), lit(&ws.join(" OR ")))
            }
        }
        Target::Date(c) => {
            let ts: Vec<String> = vals.iter().map(|v| parse_date(v).map(|t| lit(&strata::timestamp::format(t))).map_err(|why| OsError::parsing(format!("failed to parse date [{v}] of [{f}]: {why}")))).collect::<Result<_, _>>()?;
            format!("{} IN ({})", ident(&c), ts.join(", "))
        }
        Target::Vector(_) => return Err(OsError::parsing(format!("[terms] does not take the vector field [{f}]"))),
        Target::Json(p, k) => {
            let list: Vec<String> = vals.iter().map(|v| value_sql(&coerce(v, k.as_ref()))).collect::<Result<_, _>>()?;
            format!("{} IN ({})", json_path(&p), list.join(", "))
        }
    };
    Ok(Compiled { pred: Some(pred), score: Score::Const(boost(body)) })
}

/// A date in a query: a date, a time, milliseconds, or date math from
/// `now` or a date (`now-7d/d`, `2026-11-01||+1M`). `round_up` rounds a
/// `/unit` to the end of the unit, as `lte` and `gt` read it.
pub fn date_math(v: &Json, round_up: bool) -> Result<i64, String> {
    let Json::String(s) = v else { return parse_date(v) };
    let s = s.trim();
    let (mut t, rest) = if let Some(r) = s.strip_prefix("now") {
        (strata::timestamp::now(), r)
    } else if let Some((anchor, r)) = s.split_once("||") {
        (parse_date(&json!(anchor))?, r)
    } else {
        return parse_date(v);
    };
    let mut chars = rest.chars().peekable();
    while let Some(op) = chars.next() {
        let mut n = String::new();
        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
            n.push(chars.next().unwrap_or('0'));
        }
        let unit = chars.next().ok_or_else(|| format!("[{s}]: date math ends without a unit"))?;
        let u = match unit {
            'y' => strata::timestamp::Unit::Year,
            'M' => strata::timestamp::Unit::Month,
            'w' => strata::timestamp::Unit::Week,
            'd' => strata::timestamp::Unit::Day,
            'h' | 'H' => strata::timestamp::Unit::Hour,
            'm' => strata::timestamp::Unit::Minute,
            's' => strata::timestamp::Unit::Second,
            c => return Err(format!("[{s}]: date math unit [{c}]")),
        };
        match op {
            '+' | '-' => {
                let k: i64 = if n.is_empty() { 1 } else { n.parse().map_err(|_| format!("[{s}]"))? };
                let k = if op == '-' { -k } else { k };
                t = add_units(t, u, k);
            }
            '/' => {
                let start = strata::timestamp::trunc(t, u).ok_or_else(|| format!("[{s}]: out of range"))?;
                t = if round_up { add_units(start, u, 1) - 1000 } else { start };
            }
            c => return Err(format!("[{s}]: date math operator [{c}]")),
        }
    }
    Ok(t)
}

/// `t` plus `k` of `unit` (months and years by the calendar).
pub fn add_units(t: i64, unit: strata::timestamp::Unit, k: i64) -> i64 {
    use strata::timestamp::Unit;
    const S: i64 = 1_000_000;
    match unit {
        Unit::Second => t + k * S,
        Unit::Minute => t + k * 60 * S,
        Unit::Hour => t + k * 3600 * S,
        Unit::Day => t + k * 86_400 * S,
        Unit::Week => t + k * 7 * 86_400 * S,
        Unit::Month | Unit::Quarter | Unit::Year => {
            let months = match unit {
                Unit::Month => k,
                Unit::Quarter => 3 * k,
                _ => 12 * k,
            };
            add_months(t, months)
        }
    }
}

/// `t` plus `months` calendar months, the day kept (or the month's last).
fn add_months(t: i64, months: i64) -> i64 {
    const DAY: i64 = 86_400_000_000;
    let days = t.div_euclid(DAY);
    let rest = t.rem_euclid(DAY);
    let (y, m, d) = civil(days);
    let total = y * 12 + (m - 1) + months;
    let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let dim = days_in_month(ny, nm);
    days_from(ny, nm, d.min(dim)) * DAY + rest
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Howard Hinnant's civil date algorithms.
fn days_from(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn compile_range(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let (f, _, opts) = field_and(body, "range", "__none")?;
    let o = opts.as_object().ok_or_else(|| OsError::parsing("[range] query malformed"))?;
    let mut parts = Vec::new();
    let target = cx.target(&f);
    let bound = |key: &str, v: &Json| -> Result<String, OsError> {
        let up = matches!(key, "lte" | "gt");
        Ok(match &target {
            Target::Date(_) => lit(&strata::timestamp::format(date_math(v, up).map_err(|why| OsError::parsing(format!("failed to parse date field [{v}]: {why}")))?)),
            Target::Id => lit(&id_text(v)),
            Target::Json(_, k) => value_sql(&coerce(v, k.as_ref()))?,
            Target::Text(..) => value_sql(v)?,
            Target::Vector(_) => return Err(OsError::parsing(format!("[range] does not take the vector field [{f}]"))),
        })
    };
    for (k, v) in o {
        if v.is_null() {
            continue;
        }
        let op = match k.as_str() {
            "gt" => ">",
            "gte" | "from" => ">=",
            "lt" => "<",
            "lte" | "to" => "<=",
            "boost" | "format" | "time_zone" | "relation" | "include_lower" | "include_upper" => continue,
            other => return Err(OsError::parsing(format!("[range] query does not support [{other}]"))),
        };
        let op = match (k.as_str(), o.get("include_lower").and_then(Json::as_bool), o.get("include_upper").and_then(Json::as_bool)) {
            ("from", Some(false), _) => ">",
            ("to", _, Some(false)) => "<",
            _ => op,
        };
        parts.push(format!("{} {op} {}", target.sql(), bound(k, v)?));
    }
    if parts.is_empty() {
        parts.push(format!("{} IS NOT NULL", target.sql()));
    }
    Ok(Compiled { pred: and(parts), score: Score::Const(boost(&opts)) })
}

#[derive(Clone, Copy, PartialEq)]
enum MatchKind {
    Words,
    Phrase,
    PhrasePrefix,
    BoolPrefix,
}

/// A `match`-family query on one field.
fn match_one(cx: &mut Ctx, field: &str, text: &Json, opts: &Json, mk: MatchKind) -> Result<Compiled, OsError> {
    let b = boost(opts);
    let s = id_text(text);
    let target = cx.target(field);
    let (c, a) = match target {
        Target::Text(c, a) => (c, a),
        // On a keyword, number, date or the key: the value itself.
        Target::Vector(_) => return Err(OsError::parsing(format!("[match] does not take the vector field [{field}]: use [knn]"))),
        _ => {
            if mk == MatchKind::PhrasePrefix || mk == MatchKind::BoolPrefix {
                return compile(&json!({"prefix": {field: {"value": s, "boost": b}}}), cx);
            }
            return compile(&json!({"term": {field: {"value": text, "boost": b}}}), cx);
        }
    };
    let all = words_of(&s);
    let ws = kept(&all, a);
    let zero_all = opts.get("zero_terms_query").and_then(Json::as_str) == Some("all");
    if ws.is_empty() {
        return Ok(Compiled { pred: if zero_all { None } else { Some(NONE.into()) }, score: Score::None });
    }
    match mk {
        // The prefix highlights the words it starts.
        MatchKind::PhrasePrefix | MatchKind::BoolPrefix => {
            let mut noted = kept(&all[..all.len() - 1], a);
            noted.push(format!("{}*", normalize_word(&all[all.len() - 1])));
            cx.note_words(field, &noted);
        }
        _ => cx.note_words(field, &ws),
    }
    let fuzzy = opts.get("fuzziness").is_some_and(|f| !f.is_null() && f != "0" && f != 0);
    let tilde = |w: &str| if fuzzy { format!("{w}~") } else { w.to_string() };
    let q = match mk {
        MatchKind::Words => {
            let op_and = opts.get("operator").and_then(Json::as_str).is_some_and(|o| o.eq_ignore_ascii_case("and"));
            let items: Vec<String> = ws.iter().map(|w| tilde(w)).collect();
            if op_and {
                items.join(" ")
            } else if let Some(m) = opts.get("minimum_should_match") {
                let n = min_should(m, items.len())?.max(1);
                if n >= items.len() { items.join(" ") } else if n == 1 { items.join(" OR ") } else { format!("({})@{n}", items.join(" ")) }
            } else {
                items.join(" OR ")
            }
        }
        MatchKind::Phrase => {
            if all.len() == 1 { ws[0].clone() } else { format!("\"{}\"", all.join(" ")) }
        }
        MatchKind::PhrasePrefix => {
            let last = all.last().cloned().unwrap_or_default();
            let head = &all[..all.len() - 1];
            // A phrase whose last word is a prefix (Strata's `"quick bro*"`).
            match head.len() {
                0 => format!("{last}*"),
                _ => format!("\"{} {last}*\"", head.join(" ")),
            }
        }
        MatchKind::BoolPrefix => {
            let last = all.last().cloned().unwrap_or_default();
            let mut items: Vec<String> = kept(&all[..all.len() - 1], a);
            items.push(format!("{last}*"));
            items.join(" OR ")
        }
    };
    // A prefix's own word is not a term: the words before it score, and a
    // prefix alone scores its boost, as OpenSearch's prefix query does.
    let scored: Vec<String> = match mk {
        MatchKind::PhrasePrefix | MatchKind::BoolPrefix => kept(&all[..all.len() - 1], a),
        _ => ws.clone(),
    };
    let score = if scored.is_empty() { Score::Const(b) } else { Score::Expr(cx.bm25(&c, &scored, fuzzy)).scale(b) };
    Ok(Compiled { pred: Some(format!("MATCH({}, {})", ident(&c), lit(&q))), score })
}

fn compile_match(body: &Json, cx: &mut Ctx, mk: MatchKind) -> Result<Compiled, OsError> {
    let what = match mk {
        MatchKind::Words => "match",
        MatchKind::Phrase => "match_phrase",
        MatchKind::PhrasePrefix => "match_phrase_prefix",
        MatchKind::BoolPrefix => "match_bool_prefix",
    };
    let (f, text, opts) = field_and(body, what, "query")?;
    match_one(cx, &f, &text, &opts, mk)
}

fn compile_multi_match(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let text = body.get("query").ok_or_else(|| OsError::parsing("[multi_match] requires query"))?;
    let fields = cx.text_fields(body.get("fields"))?;
    if fields.is_empty() {
        return Ok(Compiled { pred: Some(NONE.into()), score: Score::None });
    }
    let ty = body.get("type").and_then(Json::as_str).unwrap_or("best_fields");
    let tie = body.get("tie_breaker").and_then(Json::as_f64).unwrap_or(if ty == "most_fields" { 1.0 } else { 0.0 });
    let mk = match ty {
        "phrase" => MatchKind::Phrase,
        "phrase_prefix" => MatchKind::PhrasePrefix,
        "bool_prefix" => MatchKind::BoolPrefix,
        "best_fields" | "most_fields" | "cross_fields" => MatchKind::Words,
        other => return Err(OsError::parsing(format!("[multi_match] query does not support type {other}"))),
    };
    let op_and = body.get("operator").and_then(Json::as_str).is_some_and(|o| o.eq_ignore_ascii_case("and"));
    if ty == "cross_fields" && op_and {
        // Every word in one of the fields.
        let ws = words_of(&id_text(text));
        let mut preds = Vec::new();
        let mut scores = Vec::new();
        for w in &ws {
            let mut per = Vec::new();
            for (f, _) in &fields {
                let c = match_one(cx, f, &json!(w), &json!({}), MatchKind::Words)?;
                per.extend(c.pred.filter(|p| p != NONE));
            }
            preds.push(or(per));
        }
        for (f, b) in &fields {
            scores.push(match_one(cx, f, text, &json!({"boost": b}), MatchKind::Words)?.score);
        }
        return Ok(Compiled { pred: and(preds), score: scores.into_iter().fold(Score::None, Score::add).scale(boost(body)) });
    }
    let mut opts = Map::new();
    for k in ["operator", "minimum_should_match", "fuzziness", "zero_terms_query"] {
        if let Some(v) = body.get(k) {
            opts.insert(k.into(), v.clone());
        }
    }
    let mut preds = Vec::new();
    let mut scores = Vec::new();
    let mut all = false;
    for (f, b) in &fields {
        let mut o = opts.clone();
        o.insert("boost".into(), json!(b));
        let c = match_one(cx, f, text, &Json::Object(o), mk)?;
        match c.pred {
            Some(p) if p == NONE => {}
            Some(p) => preds.push(p),
            None => all = true,
        }
        scores.push(c.score);
    }
    let score = if ty == "most_fields" { scores.into_iter().fold(Score::None, Score::add) } else { Score::best(scores, tie) };
    Ok(Compiled { pred: if all { None } else { Some(or(preds)) }, score: score.scale(boost(body)) })
}

// ------------------------------------------------------------------ query_string

#[derive(Debug, Clone)]
enum Qs {
    Leaf { field: Option<String>, text: String, phrase: bool },
    Range { field: String, lo: Option<String>, hi: Option<String>, lo_in: bool, hi_in: bool },
    Exists(String),
    Or(Vec<Qs>),
    /// `+x`: required, `-x`: excluded, others optional.
    Signed(Vec<(char, Qs)>),
}

struct QsParser<'a> {
    s: &'a [char],
    at: usize,
    simple: bool,
}

impl QsParser<'_> {
    fn ws(&mut self) {
        while self.at < self.s.len() && self.s[self.at].is_whitespace() {
            self.at += 1;
        }
    }

    fn peek_word(&self, w: &str) -> bool {
        let n = w.chars().count();
        if self.at + n > self.s.len() {
            return false;
        }
        let got: String = self.s[self.at..self.at + n].iter().collect();
        got == w && self.s.get(self.at + n).is_none_or(|c| c.is_whitespace() || *c == '(')
    }

    fn or(&mut self, default_and: bool) -> Qs {
        let mut parts = vec![self.and(default_and)];
        loop {
            self.ws();
            if self.peek_word("OR") {
                self.at += 2;
            } else if self.simple && self.s.get(self.at) == Some(&'|') {
                self.at += 1;
            } else if self.s.get(self.at..self.at + 2) == Some(&['|', '|']) {
                self.at += 2;
            } else {
                break;
            }
            parts.push(self.and(default_and));
        }
        if parts.len() == 1 { parts.pop().expect("one") } else { Qs::Or(parts) }
    }

    fn and(&mut self, default_and: bool) -> Qs {
        let mut items: Vec<(char, Qs)> = Vec::new();
        let mut explicit_and = false;
        loop {
            self.ws();
            if self.at >= self.s.len() || self.s[self.at] == ')' || self.peek_word("OR") || (self.simple && self.s[self.at] == '|') || self.s.get(self.at..self.at + 2) == Some(&['|', '|']) {
                break;
            }
            if self.peek_word("AND") {
                self.at += 3;
                explicit_and = true;
                continue;
            }
            if self.s.get(self.at..self.at + 2) == Some(&['&', '&']) || (self.simple && self.s[self.at] == '+' && self.s.get(self.at + 1).is_some_and(|c| c.is_whitespace())) {
                self.at += 2;
                explicit_and = true;
                continue;
            }
            let mut sign = ' ';
            if self.peek_word("NOT") {
                self.at += 3;
                sign = '-';
            } else if matches!(self.s[self.at], '+' | '-' | '!') {
                sign = if self.s[self.at] == '+' { '+' } else { '-' };
                self.at += 1;
            }
            let Some(u) = self.unary() else { break };
            items.push((sign, u));
        }
        if explicit_and || default_and {
            items.iter_mut().for_each(|(s, _)| {
                if *s == ' ' {
                    *s = '+';
                }
            });
        }
        if items.len() == 1 && items[0].0 == ' ' {
            return items.pop().expect("one").1;
        }
        Qs::Signed(items)
    }

    fn unary(&mut self) -> Option<Qs> {
        self.ws();
        let c = *self.s.get(self.at)?;
        if c == '(' {
            self.at += 1;
            let q = self.or(false);
            self.ws();
            if self.s.get(self.at) == Some(&')') {
                self.at += 1;
            }
            return Some(q);
        }
        if c == '"' {
            return Some(Qs::Leaf { field: None, text: self.quoted(), phrase: true });
        }
        // A term, or field:term.
        let start = self.at;
        let mut esc = false;
        while self.at < self.s.len() {
            let c = self.s[self.at];
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c.is_whitespace() || c == '(' || c == ')' || c == '"' || (c == ':' && !self.simple) || (self.simple && (c == '|' || c == '+')) {
                break;
            }
            self.at += 1;
        }
        let word: String = self.s[start..self.at].iter().filter(|c| **c != '\\').collect();
        if !self.simple && self.s.get(self.at) == Some(&':') {
            self.at += 1;
            let field = word;
            self.ws();
            match self.s.get(self.at) {
                Some('"') => return Some(Qs::Leaf { field: Some(field), text: self.quoted(), phrase: true }),
                Some('(') => {
                    self.at += 1;
                    let inner = self.or(false);
                    self.ws();
                    if self.s.get(self.at) == Some(&')') {
                        self.at += 1;
                    }
                    return Some(with_field(inner, &field));
                }
                Some('[') | Some('{') => {
                    let lo_in = self.s[self.at] == '[';
                    self.at += 1;
                    let start = self.at;
                    while self.at < self.s.len() && !matches!(self.s[self.at], ']' | '}') {
                        self.at += 1;
                    }
                    let inner: String = self.s[start..self.at].iter().collect();
                    let hi_in = self.s.get(self.at) == Some(&']');
                    self.at += 1;
                    let (lo, hi) = inner.split_once(" TO ").map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).unwrap_or((inner.trim().into(), "*".into()));
                    let lo = (lo != "*").then_some(lo);
                    let hi = (hi != "*").then_some(hi);
                    return Some(Qs::Range { field, lo, hi, lo_in, hi_in });
                }
                _ => {}
            }
            let start = self.at;
            while self.at < self.s.len() && !self.s[self.at].is_whitespace() && self.s[self.at] != ')' {
                self.at += 1;
            }
            let v: String = self.s[start..self.at].iter().collect();
            if field == "_exists_" {
                return Some(Qs::Exists(v));
            }
            for (op, lo_in) in [(">=", true), ("<=", true), (">", false), ("<", false)] {
                if let Some(x) = v.strip_prefix(op) {
                    let up = op.starts_with('<');
                    return Some(if up { Qs::Range { field, lo: None, hi: Some(x.into()), lo_in: true, hi_in: lo_in } } else { Qs::Range { field, lo: Some(x.into()), hi: None, lo_in, hi_in: true } });
                }
            }
            return Some(Qs::Leaf { field: Some(field), text: v, phrase: false });
        }
        if word.is_empty() {
            self.at += 1;
            return None;
        }
        Some(Qs::Leaf { field: None, text: word, phrase: false })
    }

    /// A `simple_query_string` as Lucene's simple query parser reads it:
    /// from left to right, `+` joining the next item to what came before
    /// with AND, `|` with OR, and whitespace with the default operator;
    /// `-` making the item after it every document but its. There is no
    /// precedence: what came before is one side of each operator, so that
    /// with OR, `fox +dog -lazy` is `(fox AND dog) OR NOT lazy`.
    fn simple_seq(&mut self, default_and: bool) -> Qs {
        let mut top: Option<Qs> = None;
        let mut op: Option<bool> = None;
        let mut not = false;
        loop {
            self.ws();
            let Some(&c) = self.s.get(self.at) else { break };
            match c {
                ')' => break,
                '|' | '+' => {
                    self.at += 1;
                    op = Some(c == '+');
                    continue;
                }
                '-' => {
                    self.at += 1;
                    not = !not;
                    continue;
                }
                _ => {}
            }
            let item = if c == '(' {
                self.at += 1;
                let q = self.simple_seq(default_and);
                self.ws();
                if self.s.get(self.at) == Some(&')') {
                    self.at += 1;
                }
                q
            } else {
                match self.unary() {
                    Some(q) => q,
                    None => continue,
                }
            };
            let item = if std::mem::take(&mut not) { Qs::Signed(vec![('-', item)]) } else { item };
            top = Some(match top.take() {
                None => item,
                Some(t) if op.unwrap_or(default_and) => Qs::Signed(vec![('+', t), ('+', item)]),
                Some(t) => Qs::Or(vec![t, item]),
            });
            op = None;
        }
        top.unwrap_or(Qs::Signed(Vec::new()))
    }

    fn quoted(&mut self) -> String {
        self.at += 1;
        let start = self.at;
        while self.at < self.s.len() && self.s[self.at] != '"' {
            self.at += 1;
        }
        let t: String = self.s[start..self.at].iter().collect();
        self.at += 1;
        // A slop (`"a b"~2`) is read and not kept.
        if self.s.get(self.at) == Some(&'~') {
            self.at += 1;
            while self.s.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        t
    }
}

fn with_field(q: Qs, field: &str) -> Qs {
    match q {
        Qs::Leaf { field: None, text, phrase } => Qs::Leaf { field: Some(field.into()), text, phrase },
        Qs::Or(xs) => Qs::Or(xs.into_iter().map(|x| with_field(x, field)).collect()),
        Qs::Signed(xs) => Qs::Signed(xs.into_iter().map(|(s, x)| (s, with_field(x, field))).collect()),
        q => q,
    }
}

fn compile_qs(q: &Qs, cx: &mut Ctx, fields: &[(String, f64)]) -> Result<Compiled, OsError> {
    match q {
        Qs::Leaf { field, text, phrase } => {
            let one = |cx: &mut Ctx, f: &str, b: f64| -> Result<Compiled, OsError> {
                if text == "*" {
                    return compile(&json!({"exists": {"field": f}}), cx);
                }
                if !phrase && text.ends_with('*') && !text[..text.len() - 1].contains('*') {
                    return match cx.target(f) {
                        Target::Text(..) => match_one(cx, f, &json!(&text[..text.len() - 1]), &json!({"boost": b}), MatchKind::PhrasePrefix),
                        _ => compile(&json!({"prefix": {f: {"value": &text[..text.len() - 1], "boost": b}}}), cx),
                    };
                }
                let (t, fuzzy) = match text.rsplit_once('~') {
                    Some((w, n)) if !phrase && n.chars().all(|c| c.is_ascii_digit()) => (w.to_string(), true),
                    _ => (text.clone(), false),
                };
                let mut o = json!({"boost": b});
                if fuzzy {
                    o["fuzziness"] = json!("AUTO");
                }
                match_one(cx, f, &json!(t), &o, if *phrase { MatchKind::Phrase } else { MatchKind::Words })
            };
            match field {
                Some(f) if f.contains('*') => {
                    let fs = cx.text_fields(Some(&json!(f)))?;
                    let mut cs = Vec::new();
                    for (f, b) in fs {
                        cs.push(one(cx, &f, b)?);
                    }
                    Ok(union(cs))
                }
                Some(f) => one(cx, f, 1.0),
                None => {
                    let mut cs = Vec::new();
                    for (f, b) in fields {
                        cs.push(one(cx, f, *b)?);
                    }
                    Ok(union(cs))
                }
            }
        }
        Qs::Range { field, lo, hi, lo_in, hi_in } => {
            let mut r = Map::new();
            if let Some(l) = lo {
                r.insert(if *lo_in { "gte" } else { "gt" }.into(), num_or_text(l));
            }
            if let Some(h) = hi {
                r.insert(if *hi_in { "lte" } else { "lt" }.into(), num_or_text(h));
            }
            compile(&json!({"range": {field: r}}), cx)
        }
        Qs::Exists(f) => compile(&json!({"exists": {"field": f}}), cx),
        Qs::Or(xs) => {
            let mut cs = Vec::new();
            for x in xs {
                cs.push(compile_qs(x, cx, fields)?);
            }
            Ok(union(cs))
        }
        Qs::Signed(items) => {
            let mut required = Vec::new();
            let mut optional = Vec::new();
            let mut score = Score::None;
            for (sign, x) in items {
                let saved = (*sign == '-').then(|| (cx.words.clone(), cx.scored.len()));
                let c = compile_qs(x, cx, fields)?;
                if let Some((w, n)) = saved {
                    cx.words = w;
                    cx.scored.truncate(n);
                }
                match sign {
                    '+' => {
                        required.extend(c.pred);
                        score = score.add(c.score);
                    }
                    '-' => required.push(c.pred.map_or(NONE.to_string(), |p| format!("NOT ({p})"))),
                    _ => {
                        optional.push(c.pred);
                        score = score.add(c.score);
                    }
                }
            }
            let has_positive = items.iter().any(|(s, _)| *s != '-');
            if !optional.is_empty() && required.iter().all(|p| p.starts_with("NOT (") || p == NONE) {
                if optional.iter().any(Option::is_none) {
                } else {
                    required.push(or(optional.into_iter().flatten().collect()));
                }
            }
            if !has_positive {
                score = Score::Const(1.0);
            }
            Ok(Compiled { pred: and(required), score })
        }
    }
}

fn num_or_text(s: &str) -> Json {
    s.parse::<i64>().map(|i| json!(i)).or_else(|_| s.parse::<f64>().map(|f| json!(f))).unwrap_or_else(|_| json!(s))
}

/// Any of several compiled queries, scored by their sum.
fn union(cs: Vec<Compiled>) -> Compiled {
    let mut preds = Vec::new();
    let mut all = false;
    let mut score = Score::None;
    for c in cs {
        match c.pred {
            Some(p) if p == NONE => {}
            Some(p) => preds.push(p),
            None => all = true,
        }
        score = score.add(c.score);
    }
    Compiled { pred: if all { None } else { Some(or(preds)) }, score }
}

fn compile_query_string(body: &Json, cx: &mut Ctx, simple: bool) -> Result<Compiled, OsError> {
    let text = body.get("query").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[query_string] requires query"))?;
    let fields = match body.get("fields").or_else(|| body.get("default_field")) {
        Some(f) => cx.text_fields(Some(f))?,
        None => cx.text_fields(None)?,
    };
    let default_and = body.get("default_operator").and_then(Json::as_str).is_some_and(|o| o.eq_ignore_ascii_case("and"));
    let chars: Vec<char> = text.chars().collect();
    let mut p = QsParser { s: &chars, at: 0, simple };
    let q = if simple { p.simple_seq(default_and) } else { p.or(default_and) };
    let c = compile_qs(&q, cx, &fields)?;
    Ok(Compiled { pred: c.pred, score: c.score.scale(boost(body)) })
}

// ------------------------------------------------------------------ vectors and functions

fn compile_knn(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let m = body.as_object().ok_or_else(|| OsError::parsing("[knn] query malformed"))?;
    let (f, spec) = m.iter().next().ok_or_else(|| OsError::parsing("[knn] query requires a field"))?;
    let column = match cx.target(f) {
        Target::Vector(c) => c,
        _ => return Err(OsError::parsing(format!("Field '{f}' is not knn_vector type."))),
    };
    let v = spec.get("vector").and_then(Json::as_array).ok_or_else(|| OsError::parsing("[knn] requires 'vector'"))?;
    let vals: Vec<f32> = v.iter().map(|x| x.as_f64().map(|f| f as f32)).collect::<Option<_>>().ok_or_else(|| OsError::parsing("[knn] vector is numbers"))?;
    let k = spec.get("k").and_then(Json::as_u64).unwrap_or(10) as usize;
    cx.knn_k = Some(cx.knn_k.map_or(k, |x| x.max(k)));
    let p = cx.param(strata::value::Value::Vector(vals.into()));
    let pred = match spec.get("filter") {
        Some(f) => unscored(cx, |cx| compile(f, cx))?.pred,
        None => None,
    };
    let exact = if cx.exact { ", exact => 1" } else { "" };
    Ok(Compiled { pred, score: Score::Expr(format!("similarity({}, {p}{exact})", ident(&column))).scale(boost(spec)) })
}

fn compile_function_score(body: &Json, cx: &mut Ctx) -> Result<Compiled, OsError> {
    let base = match body.get("query") {
        Some(q) => compile(q, cx)?,
        None => Compiled { pred: None, score: Score::Const(1.0) },
    };
    let mut fns: Vec<Json> = body.get("functions").and_then(Json::as_array).cloned().unwrap_or_default();
    for k in ["field_value_factor", "gauss", "exp", "linear", "weight", "random_score", "script_score"] {
        if let Some(v) = body.get(k) {
            fns.push(json!({k: v}));
        }
    }
    let mut factors = Vec::new();
    for f in &fns {
        if f.get("filter").is_some() {
            return Err(OsError::parsing("Strata's function_score takes functions without a filter"));
        }
        let weight = f.get("weight").and_then(Json::as_f64);
        let mut factor = None;
        if let Some(fv) = f.get("field_value_factor") {
            let field = fv.get("field").and_then(Json::as_str).ok_or_else(|| OsError::parsing("[field_value_factor] requires field"))?;
            let x = format!("coalesce({}, {})", cx.target(field).sql(), num(fv.get("missing").and_then(Json::as_f64).unwrap_or(1.0)));
            let x = format!("{} * {x}", num(fv.get("factor").and_then(Json::as_f64).unwrap_or(1.0)));
            factor = Some(match fv.get("modifier").and_then(Json::as_str).unwrap_or("none") {
                "none" => x,
                "log" => format!("log({x})"),
                "log1p" => format!("log(1 + {x})"),
                "log2p" => format!("log(2 + {x})"),
                "ln" => format!("ln({x})"),
                "ln1p" => format!("ln(1 + {x})"),
                "ln2p" => format!("ln(2 + {x})"),
                "square" => format!("pow({x}, 2)"),
                "sqrt" => format!("sqrt({x})"),
                "reciprocal" => format!("1 / ({x})"),
                m => return Err(OsError::parsing(format!("[field_value_factor] modifier [{m}]"))),
            });
        }
        for shape in ["gauss", "exp", "linear"] {
            if let Some(d) = f.get(shape) {
                let (field, spec) = d.as_object().and_then(|m| m.iter().find(|(k, _)| !matches!(k.as_str(), "multi_value_mode"))).ok_or_else(|| OsError::parsing(format!("[{shape}] requires a field")))?;
                let t = cx.target(field);
                let is_date = matches!(t, Target::Date(_));
                let scale_of = |v: &Json| -> Result<f64, OsError> {
                    if is_date {
                        let s = v.as_str().unwrap_or_default();
                        duration_secs(s).ok_or_else(|| OsError::parsing(format!("[{shape}] scale [{s}] is a duration such as 7d")))
                    } else {
                        v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or_else(|| OsError::parsing(format!("[{shape}] needs a scale")))
                    }
                };
                let scale = scale_of(spec.get("scale").ok_or_else(|| OsError::parsing(format!("[{shape}] needs a scale")))?)?;
                let offset = match spec.get("offset") {
                    Some(o) => scale_of(o)?,
                    None => 0.0,
                };
                let origin = match (is_date, spec.get("origin")) {
                    (true, None) => "now()".to_string(),
                    (true, Some(o)) if o.as_str() == Some("now") => "now()".to_string(),
                    (true, Some(o)) => lit(&strata::timestamp::format(date_math(o, false).map_err(|w| OsError::parsing(format!("[{shape}] origin: {w}")))?)),
                    (false, Some(o)) => num(o.as_f64().ok_or_else(|| OsError::parsing(format!("[{shape}] origin is a number")))?),
                    (false, None) => return Err(OsError::parsing(format!("[{shape}] needs an origin"))),
                };
                let decay = spec.get("decay").and_then(Json::as_f64).unwrap_or(0.5);
                factor = Some(format!("decay({}, {origin}, {}, offset => {}, decay => {}, shape => '{shape}')", t.sql(), num(scale), num(offset), num(decay)));
            }
        }
        if f.get("random_score").is_some() || f.get("script_score").is_some() {
            return Err(OsError::parsing("Strata's function_score takes field_value_factor, gauss, exp, linear and weight"));
        }
        let fx = match (factor, weight) {
            (Some(x), Some(w)) => format!("{} * {x}", num(w)),
            (Some(x), None) => x,
            (None, Some(w)) => num(w),
            (None, None) => continue,
        };
        factors.push(fx);
    }
    if factors.is_empty() {
        return Ok(Compiled { pred: base.pred, score: base.score.scale(boost(body)) });
    }
    let combined = match body.get("score_mode").and_then(Json::as_str).unwrap_or("multiply") {
        "multiply" => factors.iter().map(|f| format!("({f})")).collect::<Vec<_>>().join(" * "),
        "sum" => factors.iter().map(|f| format!("({f})")).collect::<Vec<_>>().join(" + "),
        "avg" => format!("({}) / {}", factors.join(" + "), factors.len()),
        "max" => format!("greatest({})", factors.join(", ")),
        "min" => format!("least({})", factors.join(", ")),
        "first" => factors[0].clone(),
        m => return Err(OsError::parsing(format!("[function_score] score_mode [{m}]"))),
    };
    let q = match &base.score {
        Score::Expr(e) => e.clone(),
        Score::Const(c) => num(*c),
        Score::None => "1.0".into(),
    };
    let score = match body.get("boost_mode").and_then(Json::as_str).unwrap_or("multiply") {
        "multiply" => format!("({q}) * ({combined})"),
        "sum" => format!("({q}) + ({combined})"),
        "replace" => combined,
        "avg" => format!("(({q}) + ({combined})) / 2"),
        "max" => format!("greatest({q}, {combined})"),
        "min" => format!("least({q}, {combined})"),
        m => return Err(OsError::parsing(format!("[function_score] boost_mode [{m}]"))),
    };
    Ok(Compiled { pred: base.pred, score: Score::Expr(score).scale(boost(body)) })
}

/// A duration such as `7d`, `12h`, `30m`, `10s`, `500ms`, in seconds.
pub fn duration_secs(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let (n, u) = s.split_at(split);
    let n: f64 = n.parse().ok()?;
    Some(n * match u {
        "ms" => 0.001,
        "s" => 1.0,
        "m" => 60.0,
        "h" | "H" => 3600.0,
        "d" => 86_400.0,
        "w" => 604_800.0,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ix() -> Index {
        Index::new(
            "i",
            "os_i",
            "u",
            0,
            json!({"properties": {"title": {"type": "text", "fields": {"keyword": {"type": "keyword"}}}, "body": {"type": "text", "analyzer": "english"}, "tag": {"type": "keyword"}, "n": {"type": "long"}, "at": {"type": "date"}, "v": {"type": "knn_vector", "dimension": 2}}}),
            json!({}),
            vec![],
        )
        .unwrap()
    }

    fn sql(q: Json) -> (Option<String>, Score) {
        let ix = ix();
        let mut cx = Ctx::new(&ix);
        let c = compile(&q, &mut cx).unwrap();
        (c.pred, c.score)
    }

    #[test]
    fn terms_and_ranges() {
        assert_eq!(sql(json!({"term": {"tag": "a'b"}})).0.unwrap(), r#""_source"['tag'] = 'a''b'"#);
        assert_eq!(sql(json!({"term": {"n": "5"}})).0.unwrap(), r#""_source"['n'] = 5"#);
        assert_eq!(sql(json!({"terms": {"tag": ["x", 7]}})).0.unwrap(), r#""_source"['tag'] IN ('x', '7')"#);
        assert_eq!(sql(json!({"term": {"title.keyword": "Hi"}})).0.unwrap(), r#""_source"['title'] = 'Hi'"#);
        assert_eq!(sql(json!({"range": {"n": {"gte": 1, "lt": 10}}})).0.unwrap(), r#"("_source"['n'] >= 1) AND ("_source"['n'] < 10)"#);
        assert_eq!(sql(json!({"range": {"at": {"gte": "2026-01-01"}}})).0.unwrap(), r#""at" >= '2026-01-01T00:00:00Z'"#);
        assert_eq!(sql(json!({"ids": {"values": ["1", 2]}})).0.unwrap(), r#""_id" IN ('1', '2')"#);
    }

    #[test]
    fn full_text() {
        let (p, s) = sql(json!({"match": {"title": "Red Mug"}}));
        assert_eq!(p.unwrap(), r#"MATCH("title", 'red OR mug')"#);
        assert_eq!(s, Score::Expr(r#"bm25("title", 'red mug', syntax => 'plain')"#.into()));
        let (p, _) = sql(json!({"match": {"title": {"query": "red mug", "operator": "and"}}}));
        assert_eq!(p.unwrap(), r#"MATCH("title", 'red mug')"#);
        let (p, _) = sql(json!({"match_phrase": {"title": "red mug"}}));
        assert_eq!(p.unwrap(), r#"MATCH("title", '"red mug"')"#);
        let (p, _) = sql(json!({"match": {"body": "the"}}));
        assert_eq!(p.unwrap(), NONE);
        let (p, s) = sql(json!({"multi_match": {"query": "mug", "fields": ["title^2", "body"]}}));
        assert_eq!(p.unwrap(), r#"(MATCH("title", 'mug')) OR (MATCH("body", 'mug'))"#);
        assert!(matches!(s, Score::Expr(e) if e.starts_with("greatest(2.0 * (bm25(")));
    }

    #[test]
    fn bools() {
        let (p, s) = sql(json!({"bool": {"must": [{"match": {"title": "mug"}}], "filter": [{"term": {"tag": "x"}}], "must_not": [{"term": {"n": 3}}], "should": [{"match": {"body": "red"}}]}}));
        assert_eq!(p.unwrap(), r#"(MATCH("title", 'mug')) AND ("_source"['tag'] = 'x') AND (NOT ("_source"['n'] = 3))"#);
        assert!(matches!(s, Score::Expr(e) if e.contains("bm25(\"title\"") && e.contains("bm25(\"body\"")));
        let (p, _) = sql(json!({"bool": {"should": [{"term": {"tag": "a"}}, {"term": {"tag": "b"}}, {"term": {"tag": "c"}}], "minimum_should_match": 2}}));
        assert!(p.unwrap().starts_with("AT_LEAST(2, "));
        let (p, s) = sql(json!({"bool": {"filter": {"term": {"tag": "a"}}}}));
        assert_eq!((p.unwrap().as_str(), s), (r#""_source"['tag'] = 'a'"#, Score::Const(0.0)));
    }

    #[test]
    fn query_strings() {
        let (p, _) = sql(json!({"query_string": {"query": "title:mug AND tag:x", "default_field": "title"}}));
        assert_eq!(p.unwrap(), r#"(MATCH("title", 'mug')) AND ("_source"['tag'] = 'x')"#);
        // With OR, the default, `-blue` is every document without blue.
        let (p, _) = sql(json!({"simple_query_string": {"query": "red -blue", "fields": ["title"]}}));
        assert_eq!(p.unwrap(), r#"(MATCH("title", 'red')) OR (NOT (MATCH("title", 'blue')))"#);
        let (p, _) = sql(json!({"simple_query_string": {"query": "red -blue", "fields": ["title"], "default_operator": "and"}}));
        assert_eq!(p.unwrap(), r#"(MATCH("title", 'red')) AND (NOT (MATCH("title", 'blue')))"#);
        let (p, _) = sql(json!({"query_string": {"query": "n:[1 TO 5]"}}));
        assert_eq!(p.unwrap(), r#"("_source"['n'] >= 1) AND ("_source"['n'] <= 5)"#);
    }

    #[test]
    fn date_math_rounds() {
        let t = date_math(&json!("2026-11-08T10:30:00Z||/d"), false).unwrap();
        assert_eq!(strata::timestamp::format(t), "2026-11-08T00:00:00Z");
        let t = date_math(&json!("2026-01-31||+1M"), false).unwrap();
        assert_eq!(strata::timestamp::format(t), "2026-02-28T00:00:00Z");
        assert!(date_math(&json!("now-7d/d"), false).is_ok());
    }

    #[test]
    fn min_should_forms() {
        assert_eq!(min_should(&json!(2), 3).unwrap(), 2);
        assert_eq!(min_should(&json!(-1), 3).unwrap(), 2);
        assert_eq!(min_should(&json!("75%"), 4).unwrap(), 3);
        assert_eq!(min_should(&json!("3<90%"), 2).unwrap(), 2);
        assert_eq!(min_should(&json!("3<90%"), 10).unwrap(), 9);
    }
}
