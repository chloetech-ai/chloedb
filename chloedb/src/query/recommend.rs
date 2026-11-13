//! Recommenders: next-item counts and item co-occurrence kept from a
//! table of events as it is written, and `recommend(name, user)`, a
//! ranking of items for a user that fuses them with the similarity of the
//! user's recent items and with popularity.
//!
//! ```sql
//! CREATE RECOMMENDER shop ON events(usr, item, ts) ITEMS items(emb) OPTIONS (window = 3, history = 20);
//! SELECT id, title FROM items ORDER BY recommend('shop', :u) DESC LIMIT 10;
//! SELECT * FROM recommend('shop', :u, k => 10);   -- the items and why
//! DROP RECOMMENDER shop;
//! ```
//!
//! * **Kept as a function and a table.** `CREATE RECOMMENDER` writes a
//!   stored function `name` (its body the definition, [`MARK`] and JSON)
//!   and creates the table `<name>_counts`, of rows of three kinds: a
//!   `pair` (items `src`, `dst`: `next`, the times `dst` came right after
//!   `src` in a user's events, and `co`, the times they came within
//!   `window` events of each other), an `item` (`n`, its events) and a
//!   `user` (`recent`, the user's last `history` events as JSON). It
//!   counts the events already written, in one transaction.
//! * **Fed from writes.** Each transaction that inserts rows into the
//!   events table updates the counts in the same transaction, at commit
//!   ([`feed`]): logged, numbered and shipped to replicas with the events.
//!   A row that replaces one of the same key is not a new event, and a
//!   delete takes nothing back. No file format changes: the function and
//!   the table are ordinary ones.
//! * **Read.** `recommend` reads the user's row, the pairs of their recent
//!   items (by index), the items nearest their recent ones by `maxsim`
//!   when the recommender names an items table with a vector field, and
//!   the most popular items; it ranks the candidates by reciprocal rank
//!   fusion of the four lists, leaves out the items in the user's recent
//!   events, and rewrites the call as `ranked(...)` of the keys.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use serde::{Deserialize, Serialize};
use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, VisitMut, VisitorMut};
use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::error::{Error, Result};
use crate::query::Params;
use crate::schema::{Field, FieldType, Schema};
use crate::storage::WriteTxn;
use crate::value::Value;

/// The name of the call that ranks a user's items.
pub const NAME: &str = "recommend";
/// How a recommender's function body begins.
pub const MARK: &str = "recommender:";
/// How `CREATE RECOMMENDER` is written.
pub const CREATE_FORM: &str = "CREATE [OR REPLACE] RECOMMENDER name ON events_table(user_column, item_column, time_column) [ITEMS items_table[(vector_field)]] [OPTIONS (window = 3, history = 20)]";
/// How `DROP RECOMMENDER` is written.
pub const DROP_FORM: &str = "DROP RECOMMENDER [IF EXISTS] name";
/// How a call is written.
pub const USAGE: &str = "recommend('name', user [, k => 100, next => 1, co => 1, similar => 1, popular => 0.2]): a ranking of items for the user by a recommender CREATE RECOMMENDER made";

/// Candidates each list gives at most.
const DEPTH: usize = 200;
/// The rank constant of the fusion, as `rrf`'s.
const RRF_K: f64 = 60.0;
/// The user's last items whose vectors `maxsim` reads.
const SIMILAR_OF: usize = 8;
/// Items a call ranks at most.
pub const MAX_K: usize = 10_000;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// A recommender's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Def {
    /// Lower case.
    pub name: String,
    pub events: String,
    pub user: String,
    pub item: String,
    pub ts: String,
    /// The table of the items, and its vector field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<String>,
    /// Events apart by at most this many count as seen together.
    pub window: usize,
    /// A user's events kept to recommend from (and left out of the answer).
    pub history: usize,
}

impl Def {
    /// The table of its counts.
    pub fn counts(&self) -> String {
        counts_table(&self.name)
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
        let what = format!("recommender of {}({}, {}, {}), counts in {}", self.events, self.user, self.item, self.ts, self.counts());
        format!(
            "CREATE {}FUNCTION {}() RETURNS TEXT OPTIONS (description = {}) AS {}",
            if replace { "OR REPLACE " } else { "" },
            self.name,
            quote(&what),
            quote(&body)
        )
    }

    /// Checks the definition against the tables: the events table and its
    /// columns, and the items table and its vector.
    pub fn check(&self, catalog: &[std::sync::Arc<Schema>]) -> Result<()> {
        let find = |t: &str| catalog.iter().find(|s| s.table.eq_ignore_ascii_case(t)).ok_or_else(|| err(format!("no table {t}")));
        let ev = find(&self.events)?;
        for (c, what) in [(&self.user, "user"), (&self.item, "item")] {
            let f = ev.field(c).ok_or_else(|| err(format!("{} has no column {c}", ev.table)))?;
            if !matches!(f.ty, FieldType::Keyword | FieldType::Int) || f.multi || !f.stored {
                return Err(err(format!("{}.{c}, the {what}, is a stored single-valued KEYWORD or INT column", ev.table)));
            }
        }
        let f = ev.field(&self.ts).ok_or_else(|| err(format!("{} has no column {}", ev.table, self.ts)))?;
        if !matches!(f.ty, FieldType::Int | FieldType::Float) || f.multi || !f.stored {
            return Err(err(format!("{}.{}, the time, is a stored TIMESTAMP, INT or FLOAT column", ev.table, self.ts)));
        }
        if find(&self.counts()).is_ok() {
            return Err(err(format!("table {} exists: a recommender keeps its counts there; drop it or name the recommender otherwise", self.counts())));
        }
        if let Some(t) = &self.items {
            let it = find(t)?;
            let pk = it.primary_key.ok_or_else(|| err(format!("{t} has no primary key: the items are named by their keys")))?;
            let (k, i) = (&it.fields[pk].ty, &ev.field(&self.item).expect("checked").ty);
            if k != i {
                return Err(err(format!("{}.{} and the key of {t} are of two types", ev.table, self.item)));
            }
            if let Some(v) = &self.vector {
                let f = it.field(v).ok_or_else(|| err(format!("{t} has no column {v}")))?;
                if !matches!(f.ty, FieldType::Vector { .. }) || f.multi {
                    return Err(err(format!("{t}.{v} is not a VECTOR field of one vector")));
                }
            }
        } else if self.vector.is_some() {
            return Err(err("a vector field is one of the ITEMS table"));
        }
        if self.window == 0 || self.window > 50 || self.history == 0 || self.history > 1000 {
            return Err(err("window is from 1 to 50, history from 1 to 1000"));
        }
        Ok(())
    }

    /// The schema of its counts table.
    pub fn counts_schema(&self) -> Result<Schema> {
        Schema::new(
            &self.counts(),
            vec![
                Field::new("id", FieldType::Keyword),
                Field::new("kind", FieldType::Keyword),
                Field::new("src", FieldType::Keyword),
                Field::new("dst", FieldType::Keyword),
                Field::new("next", FieldType::Float),
                Field::new("co", FieldType::Float),
                Field::new("n", FieldType::Float),
                Field::new("recent", FieldType::Keyword).stored_only(),
            ],
            Some("id"),
        )
    }
}

/// The table of recommender `name`'s counts.
pub fn counts_table(name: &str) -> String {
    format!("{}_counts", name.to_ascii_lowercase())
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `CREATE RECOMMENDER` or `DROP RECOMMENDER`.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Create { def: Def, replace: bool },
    Drop { name: String, if_exists: bool },
}

/// `CREATE [OR REPLACE] RECOMMENDER` or `DROP RECOMMENDER` of text `text`;
/// `None` for any other statement.
pub fn parse_statement(text: &str) -> Result<Option<Statement>> {
    let head = text.trim_start();
    let first: String = head.chars().take(6).collect::<String>().to_ascii_uppercase();
    if !(first.starts_with("CREATE") || first.starts_with("DROP")) || !text.as_bytes().windows(11).any(|w| w.eq_ignore_ascii_case(b"RECOMMENDER")) {
        return Ok(None);
    }
    let Ok(all) = Tokenizer::new(&GenericDialect {}, text).tokenize() else { return Ok(None) };
    let mut toks: Vec<Token> = all.into_iter().filter(|t| !matches!(t, Token::Whitespace(_))).collect();
    if toks.last() == Some(&Token::SemiColon) {
        toks.pop();
    }
    let word = |i: usize| match toks.get(i) {
        Some(Token::Word(w)) if w.quote_style.is_none() => w.value.to_ascii_uppercase(),
        _ => String::new(),
    };
    let name_at = |i: usize| match toks.get(i) {
        Some(Token::Word(w)) => Some(w.value.clone()),
        _ => None,
    };
    let create = word(0) == "CREATE";
    let replace = create && word(1) == "OR" && word(2) == "REPLACE";
    let mut i = if replace { 3 } else { 1 };
    if word(i) != "RECOMMENDER" {
        return Ok(None);
    }
    i += 1;
    let form = if create { CREATE_FORM } else { DROP_FORM };
    let bad = |m: &str| err(format!("{m}; the form is {form}"));
    let plain = |n: &str| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !n.is_empty();
    if !create {
        let if_exists = word(i) == "IF" && word(i + 1) == "EXISTS";
        if if_exists {
            i += 2;
        }
        let name = name_at(i).filter(|n| plain(n)).ok_or_else(|| bad("expected the recommender's name"))?.to_ascii_lowercase();
        if i + 1 != toks.len() {
            return Err(bad(&format!("unexpected {}", toks[i + 1])));
        }
        return Ok(Some(Statement::Drop { name, if_exists }));
    }
    let name = name_at(i).filter(|n| plain(n)).ok_or_else(|| bad("expected the recommender's name, letters, digits and _"))?.to_ascii_lowercase();
    i += 1;
    if word(i) != "ON" {
        return Err(bad("expected ON events_table(user, item, time)"));
    }
    let events = name_at(i + 1).ok_or_else(|| bad("expected the events table after ON"))?;
    i += 2;
    let mut cols = Vec::new();
    if toks.get(i) != Some(&Token::LParen) {
        return Err(bad("expected the user, item and time columns in parentheses after the events table"));
    }
    i += 1;
    loop {
        cols.push(name_at(i).ok_or_else(|| bad("expected a column"))?);
        i += 1;
        match toks.get(i) {
            Some(Token::Comma) => i += 1,
            Some(Token::RParen) => {
                i += 1;
                break;
            }
            _ => return Err(bad("expected , or ) after a column")),
        }
    }
    let [user, item, ts] = <[String; 3]>::try_from(cols).map_err(|_| bad("expected three columns: the user, the item and the time"))?;
    let (mut items, mut vector) = (None, None);
    if word(i) == "ITEMS" {
        items = Some(name_at(i + 1).ok_or_else(|| bad("expected the items table after ITEMS"))?);
        i += 2;
        if toks.get(i) == Some(&Token::LParen) {
            vector = Some(name_at(i + 1).ok_or_else(|| bad("expected the items' vector field"))?);
            if toks.get(i + 2) != Some(&Token::RParen) {
                return Err(bad("expected ) after the vector field"));
            }
            i += 3;
        }
    }
    let mut def = Def { name, events, user, item, ts, items, vector, window: 3, history: 20 };
    if word(i) == "OPTIONS" {
        if toks.get(i + 1) != Some(&Token::LParen) {
            return Err(bad("expected OPTIONS (name = value, ...)"));
        }
        i += 2;
        loop {
            let key = word(i).to_ascii_lowercase();
            let value = match (toks.get(i + 1), toks.get(i + 2)) {
                (Some(Token::Eq), Some(Token::Number(n, _))) => n.parse::<usize>().map_err(|_| bad(&format!("{key} is a whole number")))?,
                _ => return Err(bad("expected an option, name = number")),
            };
            match key.as_str() {
                "window" => def.window = value,
                "history" => def.history = value,
                k => return Err(bad(&format!("unknown option {k}"))),
            }
            i += 3;
            match toks.get(i) {
                Some(Token::Comma) => i += 1,
                Some(Token::RParen) => {
                    i += 1;
                    break;
                }
                _ => return Err(bad("expected , or ) after an option")),
            }
        }
    }
    if i != toks.len() {
        return Err(bad(&format!("unexpected {}", toks[i])));
    }
    Ok(Some(Statement::Create { def, replace }))
}

// ---- Counting ----

/// A value of a user or item column as the counts name it.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        Value::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

fn time_of(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) | Value::Timestamp(n) => Some(*n),
        Value::Float(x) if x.is_finite() => Some(*x as i64),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn pair_key(a: &str, b: &str) -> String {
    serde_json::to_string(&("p", a, b)).expect("strings")
}

fn item_key(i: &str) -> String {
    serde_json::to_string(&("i", i)).expect("strings")
}

/// The key of a user's row.
pub fn user_key(u: &str) -> String {
    serde_json::to_string(&("u", u)).expect("strings")
}

/// What events add to the counts.
#[derive(Default)]
pub struct Deltas {
    next: HashMap<(String, String), f64>,
    co: HashMap<(String, String), f64>,
    n: HashMap<String, f64>,
}

impl Deltas {
    /// Adds the event of `item` at `ts` to the user's `recent` events
    /// (oldest first, at most `d.history`), counting what it changes.
    pub fn place(&mut self, d: &Def, recent: &mut Vec<(i64, String)>, ts: Option<i64>, item: String) {
        let ts = ts.unwrap_or_else(|| recent.last().map_or(0, |r| r.0));
        let p = recent.partition_point(|(t, _)| *t <= ts);
        let before = recent.get(p.wrapping_sub(1)).map(|r| r.1.clone());
        let after = recent.get(p).map(|r| r.1.clone());
        // The pair the new event comes between is no longer adjacent.
        if let (Some(a), Some(b)) = (&before, &after) {
            if a != b {
                *self.next.entry((a.clone(), b.clone())).or_default() -= 1.0;
            }
        }
        if let Some(a) = before.as_ref().filter(|a| **a != item) {
            *self.next.entry((a.clone(), item.clone())).or_default() += 1.0;
        }
        if let Some(b) = after.as_ref().filter(|b| **b != item) {
            *self.next.entry((item.clone(), b.clone())).or_default() += 1.0;
        }
        recent.insert(p, (ts, item.clone()));
        let lo = p.saturating_sub(d.window);
        let hi = (p + d.window + 1).min(recent.len());
        let mut seen = HashSet::new();
        for (j, (_, other)) in recent[lo..hi].iter().enumerate() {
            if lo + j == p || *other == item || !seen.insert(other.clone()) {
                continue;
            }
            *self.co.entry((item.clone(), other.clone())).or_default() += 1.0;
            *self.co.entry((other.clone(), item.clone())).or_default() += 1.0;
        }
        *self.n.entry(item).or_default() += 1.0;
        if recent.len() > d.history {
            recent.drain(..recent.len() - d.history);
        }
    }

    /// Writes them, and the users' rows, into `counts` in transaction `t`.
    pub fn write(self, t: &mut WriteTxn<'_>, counts: &str, users: HashMap<String, Vec<(i64, String)>>) -> Result<()> {
        let mut c = t.table(counts)?;
        let row = |id: String, kind: &str, src: &str, dst: Option<&str>| {
            vec![Value::Str(id), Value::Str(kind.into()), Value::Str(src.into()), dst.map_or(Value::Null, |d| Value::Str(d.into())), Value::Null, Value::Null, Value::Null, Value::Null]
        };
        for (field, m) in [("next", self.next), ("co", self.co)] {
            for ((a, b), d) in m {
                if d != 0.0 {
                    c.add_or_insert(row(pair_key(&a, &b), "pair", &a, Some(&b)), field, &Value::Float(d))?;
                }
            }
        }
        for (i, d) in self.n {
            c.add_or_insert(row(item_key(&i), "item", &i, None), "n", &Value::Float(d))?;
        }
        for (u, recent) in users {
            let mut r = row(user_key(&u), "user", &u, None);
            r[7] = Value::Str(serde_json::to_string(&recent).expect("events"));
            c.upsert(r)?;
        }
        Ok(())
    }
}

/// A user's recent events, as their row holds them.
pub fn parse_recent(v: &Value) -> Vec<(i64, String)> {
    v.as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

/// The (user, item, time) of each event row, by the columns of `schema`;
/// rows without a user or an item are left out.
pub fn events_of<'a>(d: &Def, schema: &Schema, rows: impl Iterator<Item = &'a [Value]>) -> Option<Vec<(String, String, Option<i64>)>> {
    let (u, i, t) = (schema.field_index(&d.user)?, schema.field_index(&d.item)?, schema.field_index(&d.ts)?);
    Some(rows.filter_map(|r| Some((text_of(r.get(u)?)?, text_of(r.get(i)?)?, r.get(t).and_then(time_of)))).collect())
}

/// Counts the events inserted by transaction `t` into the counts of
/// recommender `d`, in `t`: at commit, before it is logged
/// ([`crate::storage::WriteTxn::commit`]). Nothing when either table is
/// gone or its columns are.
pub fn feed(t: &mut WriteTxn<'_>, d: &Def) -> Result<()> {
    let Some((schema, docs)) = t.fresh_rows(&d.events) else { return Ok(()) };
    if docs.is_empty() || t.table(&d.counts()).is_err() {
        return Ok(());
    }
    let Some(mut events) = events_of(d, &schema, docs.iter().map(|r| r.as_slice())) else { return Ok(()) };
    events.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
    let mut deltas = Deltas::default();
    let mut users: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    for (u, item, ts) in events {
        if !users.contains_key(&u) {
            let v = t.table(&d.counts())?.value(&Value::Str(user_key(&u)), "recent")?;
            users.insert(u.clone(), v.as_ref().map(parse_recent).unwrap_or_default());
        }
        deltas.place(d, users.get_mut(&u).expect("read"), ts, item);
    }
    deltas.write(t, &d.counts(), users)
}

// ---- Reading ----

/// A call's options.
#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub k: usize,
    pub next: f64,
    pub co: f64,
    pub similar: f64,
    pub popular: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { k: 100, next: 1.0, co: 1.0, similar: 1.0, popular: 0.2 }
    }
}

/// A call of `recommend` in a query.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: String,
    pub user: Value,
    pub opts: Opts,
}

/// One item recommended, and the signals it came from (its rank in each
/// list, from 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Rec {
    pub item: String,
    pub score: f64,
    pub next: Option<usize>,
    pub co: Option<usize>,
    pub similar: Option<usize>,
    pub popular: Option<usize>,
}

/// The candidate lists, each best first.
#[derive(Default)]
pub struct Lists {
    pub next: Vec<String>,
    pub co: Vec<String>,
    pub similar: Vec<String>,
    pub popular: Vec<String>,
}

/// Weighted sums of shares, best first, ties by item.
pub fn ranked_by(scores: HashMap<String, f64>) -> Vec<String> {
    let mut v: Vec<(String, f64)> = scores.into_iter().filter(|(_, s)| *s > 0.0).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().take(DEPTH).map(|(i, _)| i).collect()
}

/// The fusion of the lists: each item scores the sum over the lists of
/// weight / (60 + rank); `seen` are left out.
pub fn fuse(lists: &Lists, seen: &HashSet<String>, o: &Opts) -> Vec<Rec> {
    let mut recs: HashMap<String, Rec> = HashMap::new();
    for (list, w, slot) in [(&lists.next, o.next, 0), (&lists.co, o.co, 1), (&lists.similar, o.similar, 2), (&lists.popular, o.popular, 3)] {
        if w <= 0.0 {
            continue;
        }
        for (r, item) in list.iter().enumerate() {
            if seen.contains(item) {
                continue;
            }
            let e = recs.entry(item.clone()).or_insert_with(|| Rec { item: item.clone(), score: 0.0, next: None, co: None, similar: None, popular: None });
            let at = match slot {
                0 => &mut e.next,
                1 => &mut e.co,
                2 => &mut e.similar,
                _ => &mut e.popular,
            };
            if at.is_none() {
                *at = Some(r + 1);
                e.score += w / (RRF_K + (r + 1) as f64);
            }
        }
    }
    let mut v: Vec<Rec> = recs.into_values().collect();
    v.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.item.cmp(&b.item)));
    v.truncate(o.k);
    v
}

/// Shares of a list of (item, weight): each weight over the list's sum.
pub fn shares(rows: &[(String, f64)]) -> Vec<(String, f64)> {
    let sum: f64 = rows.iter().map(|r| r.1).filter(|w| *w > 0.0).sum();
    if sum <= 0.0 {
        return Vec::new();
    }
    rows.iter().filter(|r| r.1 > 0.0).map(|(i, w)| (i.clone(), w / sum)).collect()
}

/// The queries a call reads with, the user's recent items given.
pub struct Reads;

impl Reads {
    pub fn user(d: &Def) -> String {
        format!("SELECT recent FROM {} WHERE id = :k", crate::query::sql::ident(&d.counts()))
    }

    pub fn pairs(d: &Def, field: &str) -> String {
        format!("SELECT dst, {field} FROM {} WHERE kind = 'pair' AND src = :k AND {field} > 0 ORDER BY {field} DESC LIMIT {DEPTH}", crate::query::sql::ident(&d.counts()))
    }

    pub fn popular(d: &Def, k: usize) -> String {
        format!("SELECT src, n FROM {} WHERE kind = 'item' AND n > 0 ORDER BY n DESC LIMIT {}", crate::query::sql::ident(&d.counts()), k.min(MAX_K))
    }

    /// The items nearest the user's last ones by `maxsim`, or `None` without
    /// a vector field.
    pub fn similar(d: &Def, items: &Schema, recent: &[String]) -> Option<String> {
        let (t, v) = (d.items.as_ref()?, d.vector.as_ref()?);
        let pk = &items.fields[items.primary_key?];
        let int = pk.ty == FieldType::Int;
        let lits: Vec<String> = recent.iter().rev().take(SIMILAR_OF).filter_map(|i| if int { i.parse::<i64>().ok().map(|n| n.to_string()) } else { Some(quote(i)) }).collect();
        if lits.is_empty() {
            return None;
        }
        let (t, v, k) = (crate::query::sql::ident(t), crate::query::sql::ident(v), crate::query::sql::ident(&pk.name));
        Some(format!("SELECT {k} FROM {t} ORDER BY maxsim({v}, (SELECT {v} FROM {t} WHERE {k} IN ({}))) DESC LIMIT {DEPTH}", lits.join(", ")))
    }
}

/// The value of an item as the items' key type names it.
pub fn item_value(item: &str, int: bool) -> Value {
    match int {
        true => item.parse::<i64>().map(Value::Int).unwrap_or_else(|_| Value::Str(item.into())),
        false => Value::Str(item.into()),
    }
}

// ---- Calls in queries ----

/// Whether the text may call `recommend`.
pub fn may_recommend(sql: &str) -> bool {
    sql.as_bytes().windows(NAME.len()).any(|w| w.eq_ignore_ascii_case(NAME.as_bytes()))
}

fn is_call(f: &sql::Function) -> bool {
    f.name.0.len() == 1 && f.name.to_string().eq_ignore_ascii_case(NAME)
}

fn args_of(args: &FunctionArguments) -> Option<(Vec<Expr>, Vec<(String, Expr)>)> {
    let FunctionArguments::List(l) = args else { return None };
    let (mut pos, mut named) = (Vec::new(), Vec::new());
    for a in &l.args {
        match a {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => pos.push(e.clone()),
            FunctionArg::Named { name, arg: FunctionArgExpr::Expr(e), .. } => named.push((name.value.to_ascii_lowercase(), e.clone())),
            FunctionArg::ExprNamed { name: Expr::Identifier(name), arg: FunctionArgExpr::Expr(e), .. } => named.push((name.value.to_ascii_lowercase(), e.clone())),
            _ => return None,
        }
    }
    Some((pos, named))
}

/// The call of the arguments `args`.
fn call_of(args: &FunctionArguments, params: &Params) -> Result<Call> {
    let (pos, named) = args_of(args).ok_or_else(|| err(USAGE))?;
    let [name, user] = pos.as_slice() else { return Err(err(USAGE)) };
    let name = match crate::query::compound::value_of(name, params) {
        Ok(Value::Str(s)) => s.to_ascii_lowercase(),
        _ => match name {
            Expr::Identifier(i) => i.value.to_ascii_lowercase(),
            _ => return Err(err(format!("{USAGE}: the first argument is the recommender's name"))),
        },
    };
    let user = crate::query::compound::value_of(user, params).map_err(|e| err(format!("{USAGE}: the user is a value or a parameter ({e})")))?;
    let mut opts = Opts::default();
    for (n, e) in named {
        let v = crate::query::compound::value_of(&e, params)?;
        let x = v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| err(format!("{NAME}: {n} is a number, 0 or more")))?;
        match n.as_str() {
            "k" if x >= 1.0 && x <= MAX_K as f64 && x.fract() == 0.0 => opts.k = x as usize,
            "k" => return Err(err(format!("{NAME}: k is a whole number from 1 to {MAX_K}"))),
            "next" => opts.next = x,
            "co" => opts.co = x,
            "similar" => opts.similar = x,
            "popular" => opts.popular = x,
            o => return Err(err(format!("{NAME}: unknown option {o}; {USAGE}"))),
        }
    }
    Ok(Call { name, user, opts })
}

/// Whether the query of text `sql` calls `recommend` (in an expression or
/// in `FROM`); false for a text that does not parse as a query.
pub fn calls_sql(sql: &str, params: &Params) -> Result<bool> {
    if !may_recommend(sql) {
        return Ok(false);
    }
    let Ok(q) = crate::query::sql::parse_query(sql) else { return Ok(false) };
    Ok(table_call(&q, params)?.is_some() || !calls(&q, params)?.is_empty())
}

/// The recommenders the query of text `sql` calls, by name (in `FROM` or
/// in an expression), so that a key limited to tables can be checked for
/// their counts tables; an `Err` names a call whose recommender is not a
/// literal name.
pub fn names_sql(sql: &str) -> std::result::Result<Vec<String>, String> {
    if !may_recommend(sql) {
        return Ok(Vec::new());
    }
    let Ok(q) = crate::query::sql::parse_query(sql) else { return Ok(Vec::new()) };
    let mut names: Vec<String> = Vec::new();
    let mut push = |args: &FunctionArguments| match args_of(args).and_then(|(pos, _)| pos.into_iter().next()) {
        Some(Expr::Value(v)) => match &v.value {
            sql::Value::SingleQuotedString(n) => Ok(names.push(n.to_ascii_lowercase())),
            _ => Err(format!("{NAME}() names its recommender by a literal name")),
        },
        Some(Expr::Identifier(i)) => Ok(names.push(i.value.to_ascii_lowercase())),
        _ => Err(format!("{NAME}() names its recommender by a literal name")),
    };
    if let sql::SetExpr::Select(s) = q.body.as_ref() {
        for t in &s.from {
            if let sql::TableFactor::Table { name, args: Some(a), .. } = &t.relation {
                if name.to_string().eq_ignore_ascii_case(NAME) {
                    push(&FunctionArguments::List(sql::FunctionArgumentList { duplicate_treatment: None, args: a.args.clone(), clauses: Vec::new() }))?;
                }
            }
        }
    }
    struct Collect<'f> {
        f: &'f mut dyn FnMut(&FunctionArguments) -> std::result::Result<(), String>,
    }
    impl VisitorMut for Collect<'_> {
        type Break = String;
        fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<String> {
            if let Expr::Function(f) = e {
                if is_call(f) {
                    if let Err(m) = (self.f)(&f.args) {
                        return ControlFlow::Break(m);
                    }
                }
            }
            ControlFlow::Continue(())
        }
    }
    if let ControlFlow::Break(m) = q.clone().visit(&mut Collect { f: &mut push }) {
        return Err(m);
    }
    Ok(names)
}

/// The calls of `recommend` in a query's expressions, in order.
pub fn calls(q: &sql::Query, params: &Params) -> Result<Vec<Call>> {
    struct Collect<'p> {
        params: &'p Params,
        calls: Vec<Call>,
    }
    impl VisitorMut for Collect<'_> {
        type Break = Error;
        fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<Error> {
            if let Expr::Function(f) = e {
                if is_call(f) {
                    match call_of(&f.args, self.params) {
                        Ok(c) => self.calls.push(c),
                        Err(e) => return ControlFlow::Break(e),
                    }
                }
            }
            ControlFlow::Continue(())
        }
    }
    let mut c = Collect { params, calls: Vec::new() };
    if let ControlFlow::Break(e) = q.clone().visit(&mut c) {
        return Err(e);
    }
    Ok(c.calls)
}

/// `q` with each call of `recommend` replaced by `ranked(...)` of the keys
/// in `keys`, in the calls' order.
pub fn rewrite(q: &sql::Query, keys: &[Vec<Value>]) -> sql::Query {
    struct Rewrite<'k> {
        keys: &'k [Vec<Value>],
        at: usize,
    }
    impl VisitorMut for Rewrite<'_> {
        type Break = ();
        fn post_visit_expr(&mut self, e: &mut Expr) -> ControlFlow<()> {
            if let Expr::Function(f) = e {
                if is_call(f) {
                    let keys = self.keys.get(self.at).cloned().unwrap_or_default();
                    self.at += 1;
                    *e = ranked(&keys);
                }
            }
            ControlFlow::Continue(())
        }
    }
    let mut q = q.clone();
    let _ = q.visit(&mut Rewrite { keys, at: 0 });
    q
}

/// `ranked((k1, k2, ...))`; a list of no keys ranks no row.
fn ranked(keys: &[Value]) -> Expr {
    let lit = |v: &Value| match v {
        Value::Int(n) => Expr::value(sql::Value::Number(n.to_string(), false)),
        Value::Str(s) => Expr::value(sql::Value::SingleQuotedString(s.clone())),
        _ => Expr::value(sql::Value::Null),
    };
    let arg = match keys {
        [] => Expr::value(sql::Value::Null),
        [k] => Expr::Tuple(vec![lit(k), lit(k)]),
        ks => Expr::Tuple(ks.iter().map(lit).collect()),
    };
    let text = format!("ranked({arg})");
    match crate::query::sql::parse_query(&format!("SELECT {text}")) {
        Ok(q) => match *q.body {
            sql::SetExpr::Select(s) => match s.projection.into_iter().next() {
                Some(sql::SelectItem::UnnamedExpr(e)) => e,
                _ => Expr::value(sql::Value::Null),
            },
            _ => Expr::value(sql::Value::Null),
        },
        Err(_) => Expr::value(sql::Value::Null),
    }
}

/// `SELECT ... FROM recommend('name', user [, ...])`: the call, and the
/// query's LIMIT; `None` for any other query.
pub fn table_call(q: &sql::Query, params: &Params) -> Result<Option<(Call, Vec<String>)>> {
    let sql::SetExpr::Select(s) = q.body.as_ref() else { return Ok(None) };
    let [from] = s.from.as_slice() else { return Ok(None) };
    let sql::TableFactor::Table { name, args: Some(args), .. } = &from.relation else { return Ok(None) };
    if !name.to_string().eq_ignore_ascii_case(NAME) {
        return Ok(None);
    }
    if !from.joins.is_empty() || s.selection.is_some() || q.order_by.is_some() || !matches!(&s.group_by, sql::GroupByExpr::Expressions(g, _) if g.is_empty()) {
        return Err(err(format!("FROM {NAME}(...) is read as written, its items best first: it takes no join, WHERE, ORDER BY or GROUP BY; rank a table by ORDER BY {NAME}(...) to filter or join its rows")));
    }
    let args = FunctionArguments::List(sql::FunctionArgumentList { duplicate_treatment: None, args: args.args.clone(), clauses: Vec::new() });
    let mut call = call_of(&args, params)?;
    if let Some(sql::LimitClause::LimitOffset { limit: Some(l), offset: None, .. }) = &q.limit_clause {
        let n = crate::query::compound::value_of(l, params)?.as_f64().filter(|x| *x >= 1.0 && x.fract() == 0.0).ok_or_else(|| err("LIMIT is a whole number, 1 or more"))?;
        call.opts.k = call.opts.k.min(n as usize);
    }
    let mut cols = Vec::new();
    for item in &s.projection {
        match item {
            sql::SelectItem::Wildcard(_) => cols.extend(COLUMNS.iter().map(|c| c.to_string())),
            sql::SelectItem::UnnamedExpr(Expr::Identifier(i)) if COLUMNS.contains(&i.value.to_ascii_lowercase().as_str()) => cols.push(i.value.to_ascii_lowercase()),
            i => return Err(err(format!("FROM {NAME}(...) has the columns {}, not {i}", COLUMNS.join(", ")))),
        }
    }
    Ok(Some((call, cols)))
}

/// The columns of `FROM recommend(...)`.
pub const COLUMNS: [&str; 6] = ["item", "score", "next", "co", "similar", "popular"];

/// The rows of `FROM recommend(...)`, of columns `cols`.
pub fn table_rows(recs: &[Rec], cols: &[String], int: bool) -> Vec<Vec<Value>> {
    let rank = |r: Option<usize>| r.map_or(Value::Null, |n| Value::Int(n as i64));
    recs.iter()
        .map(|r| {
            cols.iter()
                .map(|c| match c.as_str() {
                    "item" => item_value(&r.item, int),
                    "score" => Value::Float(r.score),
                    "next" => rank(r.next),
                    "co" => rank(r.co),
                    "similar" => rank(r.similar),
                    _ => rank(r.popular),
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> Def {
        Def { name: "shop".into(), events: "events".into(), user: "usr".into(), item: "item".into(), ts: "ts".into(), items: None, vector: None, window: 2, history: 4 }
    }

    #[test]
    fn parses_the_statements() {
        let s = parse_statement("CREATE RECOMMENDER Shop ON events(usr, item, ts) ITEMS items(emb) OPTIONS (window = 5, history = 30);").unwrap().unwrap();
        let Statement::Create { def: made, replace } = s else { panic!() };
        assert!(!replace);
        assert_eq!((made.name.as_str(), made.items.as_deref(), made.vector.as_deref(), made.window, made.history), ("shop", Some("items"), Some("emb"), 5, 30));
        assert_eq!(parse_statement("DROP RECOMMENDER IF EXISTS shop").unwrap(), Some(Statement::Drop { name: "shop".into(), if_exists: true }));
        assert!(parse_statement("SELECT 1").unwrap().is_none());
        assert!(parse_statement("CREATE RECOMMENDER shop ON events(usr, item)").unwrap_err().to_string().contains("three columns"));
        let d = def();
        let sql = d.function_sql(false);
        let body = sql.split(" AS ").last().unwrap();
        assert_eq!(Def::of_body(body), Some(d));
    }

    #[test]
    fn counts_next_items_and_neighbours() {
        let d = def();
        let mut deltas = Deltas::default();
        let mut recent = Vec::new();
        for (t, i) in [(1, "a"), (2, "b"), (4, "c")] {
            deltas.place(&d, &mut recent, Some(t), i.into());
        }
        // Out of order: d between b and c.
        deltas.place(&d, &mut recent, Some(3), "d".into());
        let next = |a: &str, b: &str| deltas.next.get(&(a.to_string(), b.to_string())).copied().unwrap_or(0.0);
        assert_eq!((next("a", "b"), next("b", "c"), next("b", "d"), next("d", "c")), (1.0, 0.0, 1.0, 1.0));
        assert_eq!(recent.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["a", "b", "d", "c"]);
        assert_eq!(deltas.co.get(&("a".to_string(), "c".to_string())), Some(&1.0));
        assert_eq!(deltas.co.get(&("d".to_string(), "a".to_string())), Some(&1.0));
        // History keeps the last four.
        deltas.place(&d, &mut recent, Some(9), "e".into());
        assert_eq!(recent.len(), 4);
    }

    #[test]
    fn fuses_and_leaves_out_what_was_seen() {
        let lists = Lists { next: vec!["x".into(), "y".into()], co: vec!["y".into(), "a".into()], similar: vec![], popular: vec!["z".into(), "y".into()] };
        let seen: HashSet<String> = ["a".to_string()].into();
        let r = fuse(&lists, &seen, &Opts::default());
        assert_eq!(r.iter().map(|r| r.item.as_str()).collect::<Vec<_>>(), ["y", "x", "z"]);
        assert_eq!((r[0].next, r[0].co, r[0].popular), (Some(2), Some(1), Some(2)));
    }
}
