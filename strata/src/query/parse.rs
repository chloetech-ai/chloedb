//! Parsing SQL text into statements, with a bound on expression depth.
//!
//! The parser builds `a OR b OR c ...` as a tree as deep as the chain is
//! long, and binding, printing and even dropping that tree recurse once
//! per level. Without a bound, one long chain overflows the thread's stack
//! and aborts the process (a tokio worker has 2 MB). Like SQLite's
//! `SQLITE_MAX_EXPR_DEPTH`, statements whose expressions could nest more
//! than [`MAX_EXPR_DEPTH`] levels are refused before a tree is built.

use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Span, Token, TokenWithSpan, Tokenizer};

use crate::error::{Error, Result};

/// How deeply an expression may nest: parentheses plus the operators a
/// chain such as `a OR b OR ...` or `x + y + ...` strings together.
/// SQLite's default limit is 1,000 too.
pub const MAX_EXPR_DEPTH: usize = 1000;

/// The call `MAX k PER field` parses as, the last item of its query's
/// ORDER BY: `__max_per(k, field)` ([`crate::query::compound`]).
pub const PER_CAP: &str = "__max_per";

/// Parses `sql_text` into its statements (generic SQL dialect).
pub fn parse(sql_text: &str) -> Result<Vec<Statement>> {
    let dialect = GenericDialect {};
    let located = Tokenizer::new(&dialect, sql_text).tokenize_with_location().map_err(|e| Error::Query(format!("SQL syntax error: {e}")))?;
    let tokens = per_caps(located);
    let plain: Vec<Token> = tokens.iter().map(|t| t.token.clone()).collect();
    check_depth(&plain)?;
    Parser::new(&dialect).with_tokens_with_locations(tokens).parse_statements().map_err(|e| Error::Query(syntax_error(&e.to_string(), sql_text)))
}

/// Keywords a statement is written with, for "did you mean" a misspelt one.
const KEYWORDS: [&str; 44] = [
    "SELECT", "FROM", "WHERE", "GROUP", "ORDER", "BY", "HAVING", "LIMIT", "OFFSET", "JOIN", "LEFT", "INNER", "ON", "AS", "AND", "OR", "NOT", "IN", "IS", "NULL", "LIKE", "ILIKE", "BETWEEN",
    "DISTINCT", "WITH", "UNION", "INTERSECT", "EXCEPT", "CASE", "WHEN", "THEN", "ELSE", "END", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "EXPLAIN",
];

/// The parser's message (`sql parser error: Expected: end of statement,
/// found: FORM at Line: 1, Column: 10`) as one that says where, what came
/// instead and, for a misspelt keyword, which it may be: `SQL syntax
/// error at line 1, column 10 (near "FORM products"): expected the end of
/// the statement, found FORM (did you mean FROM?)`.
fn syntax_error(message: &str, sql_text: &str) -> String {
    let m = message.trim_start_matches("sql parser error: ");
    // The place, if the parser gave one.
    let (m, at) = match m.rfind(" at Line: ") {
        Some(i) => {
            let place = &m[i + " at Line: ".len()..];
            let mut parts = place.split(", Column: ");
            let line = parts.next().and_then(|l| l.trim().parse::<usize>().ok());
            let col = parts.next().and_then(|c| c.trim().parse::<usize>().ok());
            (&m[..i], line.zip(col))
        }
        None => (m, None),
    };
    let (expected, found) = match m.strip_prefix("Expected: ").and_then(|r| r.rsplit_once(", found: ")) {
        Some((e, f)) => (Some(e), Some(f)),
        None => (None, None),
    };
    // A misspelt clause keyword before the error reads as an alias of what
    // precedes it (`SELECT id FORM t`: FORM names id), and the parser stops
    // a word later: the place to fix is the misspelt word.
    if let (Some((line, col)), Some(f)) = (at, found) {
        if let Some((w, wl, wc, kw)) = alias_typo(sql_text, line, col) {
            let mut out = format!("SQL syntax error at line {wl}, column {wc}");
            if let Some(near) = near(sql_text, wl as usize, wc as usize) {
                out.push_str(&format!(" (near \"{near}\")"));
            }
            out.push_str(&format!(": {w} reads as a name here, the alias of what comes before it, and then {f} cannot follow (did you mean {kw}?)"));
            return out;
        }
    }
    let mut out = String::from("SQL syntax error");
    if let Some((line, col)) = at {
        out.push_str(&format!(" at line {line}, column {col}"));
        if let Some(near) = near(sql_text, line, col) {
            out.push_str(&format!(" (near \"{near}\")"));
        }
    } else if found == Some("EOF") {
        out.push_str(" at the end of the statement");
    }
    out.push_str(": ");
    match (expected, found) {
        (Some(e), Some("EOF")) => out.push_str(&format!("the statement ends where {} should follow", readable(e))),
        (Some(e), Some(f)) => {
            out.push_str(&format!("expected {}, found {f}", readable(e)));
            let word = f.trim_matches(|c| c == '"' || c == '`');
            if word.chars().all(|c| c.is_ascii_alphabetic()) && !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word)) {
                out.push_str(&super::hint::did_you_mean(&word.to_ascii_uppercase(), KEYWORDS.iter().copied()));
            }
        }
        _ => out.push_str(m),
    }
    out
}

/// Clause keywords a misspelling of which reads as an alias.
const CLAUSES: [&str; 12] = ["FROM", "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "JOIN", "UNION", "VALUES", "INTO", "SET"];

/// The word just before (`line`, `col`) when it is a name of three or more
/// letters near a clause keyword, not a keyword itself: the word, its line
/// and column, and the keyword it may be.
fn alias_typo(sql_text: &str, line: usize, col: usize) -> Option<(String, u64, u64, &'static str)> {
    let tokens = Tokenizer::new(&GenericDialect {}, sql_text).tokenize_with_location().ok()?;
    let at = tokens.iter().position(|t| t.span.start.line == line as u64 && t.span.start.column == col as u64)?;
    let prev = tokens[..at].iter().rev().find(|t| !matches!(t.token, Token::Whitespace(_)))?;
    let Token::Word(w) = &prev.token else { return None };
    if w.quote_style.is_some() || w.value.len() < 3 || !w.value.chars().all(|c| c.is_ascii_alphabetic()) || KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(&w.value)) {
        return None;
    }
    let kw = *super::hint::nearest(&w.value.to_ascii_uppercase(), CLAUSES.iter().copied()).first()?;
    Some((w.value.clone(), prev.span.start.line, prev.span.start.column, kw))
}

/// The parser's names of what it expected, as a sentence says them.
fn readable(expected: &str) -> String {
    match expected {
        "end of statement" => "the end of the statement (or ; before another)".into(),
        "an SQL statement" => "a statement such as SELECT, INSERT, UPDATE, DELETE or CREATE TABLE".into(),
        "an expression" => "an expression (a column, a value or a condition)".into(),
        "identifier" => "a name".into(),
        e => e.to_string(),
    }
}

/// The text from (`line`, `col`), both from 1, to the end of the next word.
fn near(sql_text: &str, line: usize, col: usize) -> Option<String> {
    let l = sql_text.lines().nth(line.checked_sub(1)?)?;
    let rest: String = l.chars().skip(col.saturating_sub(1)).collect();
    let rest = rest.trim_end();
    if rest.is_empty() {
        return None;
    }
    // Two words, at most 30 characters.
    let mut end = 0;
    for (n, w) in rest.split_inclusive(char::is_whitespace).enumerate() {
        if n >= 2 || end + w.len() > 30 {
            break;
        }
        end += w.len();
    }
    let cut = if end == 0 { rest.chars().take(30).collect::<String>() } else { rest[..end].trim_end().to_string() };
    Some(cut)
}

/// `MAX k PER field` (after an ORDER BY, or without one) as the call
/// [`PER_CAP`] ending the ORDER BY: `ORDER BY a DESC MAX 2 PER b` parses
/// as `ORDER BY a DESC, __max_per(2, b)`. `MAX` followed by a number is
/// nothing else in SQL. The field is a name, or names joined by dots.
fn per_caps(located: Vec<TokenWithSpan>) -> Vec<TokenWithSpan> {
    let word = |t: &Token, w: &str| matches!(t, Token::Word(x) if x.quote_style.is_none() && x.value.eq_ignore_ascii_case(w));
    let tokens: Vec<Token> = located.iter().map(|t| t.token.clone()).collect();
    // Whether the query of each open parenthesis has had an ORDER BY.
    let mut ordered = vec![false];
    let mut out: Vec<TokenWithSpan> = Vec::with_capacity(tokens.len());
    // The tokens written in place of `MAX k PER field` take its place.
    let put = |out: &mut Vec<TokenWithSpan>, span: Span, ts: Vec<Token>| out.extend(ts.into_iter().map(|token| TokenWithSpan { token, span }));
    let mut last: Option<Token> = None;
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        match t {
            Token::LParen => ordered.push(false),
            Token::RParen if ordered.len() > 1 => {
                ordered.pop();
            }
            Token::Word(w) if w.keyword == Keyword::SELECT => *ordered.last_mut().expect("the statement level") = false,
            Token::Word(w) if w.keyword == Keyword::BY && last.as_ref().is_some_and(|l| word(l, "ORDER")) => *ordered.last_mut().expect("the statement level") = true,
            _ => {}
        }
        if word(t, "MAX") {
            let next: Vec<usize> = (i + 1..tokens.len()).filter(|&j| !matches!(tokens[j], Token::Whitespace(_))).take(3).collect();
            if let [k, per, name] = next[..] {
                if matches!(tokens[k], Token::Number(..)) && word(&tokens[per], "PER") && matches!(tokens[name], Token::Word(_)) {
                    let mut end = name + 1;
                    while end + 1 < tokens.len() && tokens[end] == Token::Period && matches!(tokens[end + 1], Token::Word(_)) {
                        end += 2;
                    }
                    let span = located[i].span;
                    let level = ordered.last_mut().expect("the statement level");
                    match *level {
                        true => put(&mut out, span, vec![Token::Comma]),
                        false => put(&mut out, span, vec![Token::make_keyword("ORDER"), Token::Whitespace(sqlparser::tokenizer::Whitespace::Space), Token::make_keyword("BY")]),
                    }
                    *level = true;
                    put(&mut out, span, vec![Token::Whitespace(sqlparser::tokenizer::Whitespace::Space), Token::make_word(PER_CAP, None), Token::LParen, tokens[k].clone(), Token::Comma]);
                    out.extend(located[name..end].iter().cloned());
                    put(&mut out, located[end - 1].span, vec![Token::RParen]);
                    last = Some(Token::RParen);
                    i = end;
                    continue;
                }
            }
        }
        if !matches!(t, Token::Whitespace(_)) {
            last = Some(t.clone());
        }
        out.push(located[i].clone());
        i += 1;
    }
    out
}

/// `sql` printed from a parsed statement, its [`PER_CAP`] calls written
/// back as `MAX k PER field`.
pub fn unparse_per_caps(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(at) = rest.find(&format!("{PER_CAP}(")) {
        let args_at = at + PER_CAP.len() + 1;
        let (Some(close), Some(comma)) = (rest[args_at..].find(')'), rest[args_at..].find(", ")) else { break };
        let before = &rest[..at];
        let before = before.strip_suffix(", ").or_else(|| before.strip_suffix("ORDER BY ")).unwrap_or(before);
        out.push_str(before.trim_end());
        out.push_str(&format!(" MAX {} PER {}", &rest[args_at..args_at + comma], &rest[args_at + comma + 2..args_at + close]));
        rest = &rest[args_at + close + 1..];
    }
    out.push_str(rest);
    out
}

/// An upper bound of the expression depth `tokens` can parse into: within
/// each open parenthesis (or bracket), the operators since its last comma,
/// summed over the open ones, plus how many are open. Commas separate list
/// items, which sit side by side, so `IN (1, 2, ...)` and `VALUES (...),
/// (...)` stay shallow however long they are.
fn check_depth(tokens: &[Token]) -> Result<()> {
    let mut open: Vec<usize> = vec![0];
    let mut sum = 0usize;
    for t in tokens {
        match t {
            Token::LParen | Token::LBracket | Token::LBrace => open.push(0),
            Token::RParen | Token::RBracket | Token::RBrace => {
                if open.len() > 1 {
                    sum -= open.pop().unwrap_or(0);
                }
            }
            Token::Comma | Token::SemiColon => {
                let last = open.last_mut().expect("the statement level");
                sum -= *last;
                *last = 0;
            }
            t if chains(t) => {
                *open.last_mut().expect("the statement level") += 1;
                sum += 1;
            }
            _ => {}
        }
        if sum + open.len() > MAX_EXPR_DEPTH {
            return Err(Error::Query(format!("the statement nests expressions more than {MAX_EXPR_DEPTH} levels deep (long chains of AND, OR or arithmetic, or deep parentheses); use IN lists or split it")));
        }
    }
    Ok(())
}

/// Whether a token can join two expressions into a deeper one: every
/// operator symbol and the operator keywords. Names, literals and other
/// keywords cannot.
fn chains(t: &Token) -> bool {
    match t {
        Token::Word(w) => matches!(
            w.keyword,
            Keyword::AND
                | Keyword::OR
                | Keyword::XOR
                | Keyword::NOT
                | Keyword::IS
                | Keyword::IN
                | Keyword::BETWEEN
                | Keyword::LIKE
                | Keyword::ILIKE
                | Keyword::SIMILAR
                | Keyword::RLIKE
                | Keyword::REGEXP
                | Keyword::DIV
                | Keyword::MOD
                | Keyword::COLLATE
                | Keyword::AT
                | Keyword::OVERLAPS
                | Keyword::MEMBER
                | Keyword::ESCAPE
        ),
        Token::Whitespace(_)
        | Token::EOF
        | Token::Number(..)
        | Token::Char(_)
        | Token::SingleQuotedString(_)
        | Token::DoubleQuotedString(_)
        | Token::TripleSingleQuotedString(_)
        | Token::TripleDoubleQuotedString(_)
        | Token::DollarQuotedString(_)
        | Token::SingleQuotedByteStringLiteral(_)
        | Token::DoubleQuotedByteStringLiteral(_)
        | Token::TripleSingleQuotedByteStringLiteral(_)
        | Token::TripleDoubleQuotedByteStringLiteral(_)
        | Token::SingleQuotedRawStringLiteral(_)
        | Token::DoubleQuotedRawStringLiteral(_)
        | Token::TripleSingleQuotedRawStringLiteral(_)
        | Token::TripleDoubleQuotedRawStringLiteral(_)
        | Token::NationalStringLiteral(_)
        | Token::EscapedStringLiteral(_)
        | Token::UnicodeStringLiteral(_)
        | Token::HexStringLiteral(_)
        | Token::Placeholder(_) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_k_per_parses_as_the_last_ordering() {
        let as_sql = |q: &str| parse(q).unwrap()[0].to_string();
        assert_eq!(as_sql("SELECT id FROM t ORDER BY bm25(body, 'x') DESC MAX 2 PER venue LIMIT 20"), "SELECT id FROM t ORDER BY bm25(body, 'x') DESC, __max_per(2, venue) LIMIT 20");
        assert_eq!(as_sql("SELECT id FROM t max 1 per a.b"), "SELECT id FROM t ORDER BY __max_per(1, a.b)");
        assert_eq!(
            as_sql("SELECT id FROM t WHERE id IN (SELECT id FROM t ORDER BY n MAX 3 PER v LIMIT 5) ORDER BY MAX(n) DESC"),
            "SELECT id FROM t WHERE id IN (SELECT id FROM t ORDER BY n, __max_per(3, v) LIMIT 5) ORDER BY MAX(n) DESC"
        );
        assert_eq!(as_sql("SELECT MAX(n) FROM t"), "SELECT MAX(n) FROM t");
        for q in ["SELECT id FROM t ORDER BY n DESC MAX 2 PER venue LIMIT 20", "SELECT id FROM t WHERE id IN (SELECT id FROM t ORDER BY n MAX 3 PER a.v) LIMIT 5"] {
            assert_eq!(unparse_per_caps(&as_sql(q)), q);
        }
    }

    #[test]
    fn long_lists_pass_and_long_chains_do_not() {
        let list = (0..50_000).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        assert!(parse(&format!("SELECT id FROM t WHERE id IN ({list})")).is_ok());
        let rows = (0..10_000).map(|i| format!("({i}, -1.5, 'a')")).collect::<Vec<_>>().join(", ");
        assert!(parse(&format!("INSERT INTO t VALUES {rows}")).is_ok());
        let ok = vec!["a = 1"; 300].join(" AND ");
        assert!(parse(&format!("SELECT id FROM t WHERE {ok}")).is_ok());
        for bad in [vec!["a = 1"; 2000].join(" OR "), vec!["x"; 2000].join(" + "), format!("{}a", "NOT ".repeat(2000))] {
            let r = parse(&format!("SELECT id FROM t WHERE {bad}"));
            assert!(matches!(&r, Err(Error::Query(m)) if m.contains("nests")), "{r:?}");
        }
    }
}
