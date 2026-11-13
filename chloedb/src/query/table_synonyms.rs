//! A table's own synonym list: `CREATE SYNONYMS ON table AS 'rules'`,
//! the rules `synonyms => 1` uses on that table's searches besides the
//! server's list ([`crate::text::synonyms`]).
//!
//! ```sql
//! CREATE SYNONYMS ON products AS 'sofa, couch
//! tv => television';
//! CREATE OR REPLACE SYNONYMS ON products AS '...';   -- the next version
//! DROP SYNONYMS [IF EXISTS] ON products;
//! SELECT id FROM products WHERE MATCH(name, 'couch', synonyms => 1);
//! ```
//!
//! * **Kept as a function.** The list is the stored function
//!   `synonyms_<table>() RETURNS TEXT`, its body the rules: it has the
//!   versions, rollbacks (`CREATE OR REPLACE FUNCTION synonyms_products
//!   FROM VERSION 1`) and replication of any function, and through a
//!   coordinator every shard holds it.
//! * **Rules** are those of a synonym file (Solr's format), checked when
//!   the list is made.
//! * **Use.** A search of the table with `synonyms => 1` expands by the
//!   server's list and the table's, the table's rules after the server's;
//!   without `synonyms => 1` neither applies.

use std::sync::{Arc, Mutex, OnceLock};

use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Token, TokenWithSpan, Tokenizer};

use super::function::{Function, FunctionSet};
use crate::error::{Error, Result};
use crate::text::synonyms::Synonyms;

/// The prefix of the name of a table's synonym function.
pub const PREFIX: &str = "synonyms_";

const USAGE: &str = "CREATE [OR REPLACE] SYNONYMS ON table AS 'rules', or DROP SYNONYMS [IF EXISTS] ON table";

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// The name of the function that holds table `table`'s list.
pub fn function_name(table: &str) -> String {
    format!("{PREFIX}{}", table.to_ascii_lowercase())
}

/// A statement of a table's synonym list.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Create { table: String, replace: bool, rules: String },
    Drop { table: String, if_exists: bool },
}

impl Statement {
    pub fn table(&self) -> &str {
        match self {
            Statement::Create { table, .. } | Statement::Drop { table, .. } => table,
        }
    }

    /// The `CREATE FUNCTION` or `DROP FUNCTION` it is, its rules checked.
    pub fn function_sql(&self) -> Result<String> {
        match self {
            Statement::Create { table, replace, rules } => {
                let parsed = Synonyms::parse(rules).map_err(|e| err(format!("synonyms of {table}: {e}")))?;
                if parsed.is_empty() {
                    return Err(err(format!("synonyms of {table}: no rules (DROP SYNONYMS ON {table} removes the list)")));
                }
                Ok(format!("CREATE {}FUNCTION {}() RETURNS TEXT AS '{}'", if *replace { "OR REPLACE " } else { "" }, function_name(table), rules.replace('\'', "''")))
            }
            Statement::Drop { table, if_exists } => Ok(format!("DROP FUNCTION {}{}", if *if_exists { "IF EXISTS " } else { "" }, function_name(table))),
        }
    }
}

/// The statement text `text` is, if it is one of a table's synonyms.
pub fn parse_statement(text: &str) -> Result<Option<Statement>> {
    let head = text.trim_start();
    let first: String = head.chars().take(6).collect::<String>().to_ascii_uppercase();
    if !(first.starts_with("CREATE") || first.starts_with("DROP")) {
        return Ok(None);
    }
    let words: Vec<String> = head.split_whitespace().take(4).map(str::to_ascii_uppercase).collect();
    if !words.iter().any(|w| w == "SYNONYMS") {
        return Ok(None);
    }
    let all = Tokenizer::new(&GenericDialect {}, text).tokenize_with_location().map_err(|e| err(format!("SQL syntax error: {e}")))?;
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
    let mut i = 1;
    match word(0).as_str() {
        "CREATE" => {
            let replace = word(1) == "OR" && word(2) == "REPLACE";
            if replace {
                i = 3;
            }
            if word(i) != "SYNONYMS" || word(i + 1) != "ON" {
                return Ok(None);
            }
            let table = name_at(i + 2).ok_or_else(|| err(USAGE))?;
            if word(i + 3) != "AS" {
                return Err(err(USAGE));
            }
            let rules = match toks.get(i + 4).map(|t| &t.token) {
                Some(Token::SingleQuotedString(s)) if toks.len() == i + 5 => s.clone(),
                _ => return Err(err(format!("{USAGE}: the rules are one quoted text"))),
            };
            Ok(Some(Statement::Create { table, replace, rules }))
        }
        "DROP" => {
            if word(1) != "SYNONYMS" {
                return Ok(None);
            }
            i = 2;
            let if_exists = word(2) == "IF" && word(3) == "EXISTS";
            if if_exists {
                i = 4;
            }
            if word(i) != "ON" || toks.len() != i + 2 {
                return Err(err(USAGE));
            }
            Ok(Some(Statement::Drop { table: name_at(i + 1).ok_or_else(|| err(USAGE))?, if_exists }))
        }
        _ => Ok(None),
    }
}

/// The rules a table's synonym function holds.
pub fn rules_of(f: &Function) -> Option<String> {
    let toks = Tokenizer::new(&GenericDialect {}, &f.def.body).tokenize().ok()?;
    let mut strings = toks.into_iter().filter(|t| !matches!(t, Token::Whitespace(_)));
    match (strings.next(), strings.next()) {
        (Some(Token::SingleQuotedString(s)), None) => Some(s),
        _ => None,
    }
}

type Merged = (Arc<Function>, Option<Arc<Synonyms>>, Arc<Synonyms>);

fn cache() -> &'static Mutex<Vec<Merged>> {
    static C: OnceLock<Mutex<Vec<Merged>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// The synonym list a search of `table` expands by: `base` (the server's,
/// if any) with the table's rules after it, or `None` when the table has
/// no list of its own (then `base` alone applies). Parsed once per version
/// of the function and of `base`.
pub fn for_table(functions: &FunctionSet, table: &str, base: Option<&Arc<Synonyms>>) -> Result<Option<Arc<Synonyms>>> {
    let Some(f) = functions.current(&function_name(table)) else { return Ok(None) };
    let same_base = |b: &Option<Arc<Synonyms>>| match (b, base) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    };
    if let Some((_, _, s)) = cache().lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(g, b, _)| Arc::ptr_eq(g, f) && same_base(b)) {
        return Ok(Some(s.clone()));
    }
    let Some(rules) = rules_of(f) else { return Ok(None) };
    let text = match base {
        Some(b) if !b.text().trim().is_empty() => format!("{}\n{rules}", b.text()),
        _ => rules,
    };
    let s = Arc::new(Synonyms::parse(&text).map_err(|e| err(format!("synonyms of {table}: {e}")))?);
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    c.push((f.clone(), base.cloned(), s.clone()));
    if c.len() > 64 {
        c.remove(0);
    }
    Ok(Some(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_parse() {
        assert_eq!(
            parse_statement("CREATE OR REPLACE SYNONYMS ON products AS 'sofa, couch\ntv => it''s television';").unwrap(),
            Some(Statement::Create { table: "products".into(), replace: true, rules: "sofa, couch\ntv => it's television".into() })
        );
        assert_eq!(parse_statement("drop synonyms if exists on Products").unwrap(), Some(Statement::Drop { table: "Products".into(), if_exists: true }));
        assert_eq!(parse_statement("CREATE TABLE synonyms (id KEYWORD)").unwrap(), None);
        assert_eq!(parse_statement("DROP TABLE synonyms").unwrap(), None);
        assert!(parse_statement("CREATE SYNONYMS ON products 'a, b'").is_err());
        assert!(parse_statement("CREATE SYNONYMS ON products AS 'a, b' extra").is_err());
        let sql = parse_statement("CREATE SYNONYMS ON products AS 'it''s, its'").unwrap().unwrap().function_sql().unwrap();
        assert_eq!(sql, "CREATE FUNCTION synonyms_products() RETURNS TEXT AS 'it''s, its'");
        assert!(parse_statement("CREATE SYNONYMS ON products AS '# nothing'").unwrap().unwrap().function_sql().is_err());
    }
}
