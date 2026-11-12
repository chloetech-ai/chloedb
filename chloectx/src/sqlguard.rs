//! What SQL from outside ChloeCtx may do: `memory_sql` (a model's, which a
//! prompt or a recalled record can steer), the inspector's query box, and
//! the query engine's model-written conditions.
//!
//! Such a statement is one `SELECT` (parsed, not recognised by its first
//! word), calls no function that sends rows out of the process (`llm()`,
//! a hosted `cross_encoder()`), and runs with [`Limits`]: a deadline, a
//! memory budget and a cap on the rows it returns, so a cross join of the
//! store with itself fails in seconds instead of holding the daemon.

use std::time::Duration;

use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::{err, Result};

/// Functions a caller's SQL may not call: each sends what it reads of the
/// rows to a model, possibly a third party's API with the keys in the
/// daemon's environment.
pub const REFUSED_FUNCTIONS: &[&str] = &[chloedb::query::llm::NAME, "cross_encoder"];

/// Bounds on one statement of a caller's.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The statement fails once this has passed.
    pub timeout: Duration,
    /// The most rows it may return (or reach with LIMIT and OFFSET).
    pub max_rows: usize,
    /// The most bytes its groups, candidates and rows may hold.
    pub memory_bytes: u64,
}

impl Limits {
    /// For `memory_sql`, which shows a model the first 100 rows.
    pub const TOOL: Limits = Limits { timeout: Duration::from_secs(2), max_rows: 1000, memory_bytes: 256 << 20 };
    /// For the inspector's query box, which a person waits on.
    pub const INSPECTOR: Limits = Limits { timeout: Duration::from_secs(10), max_rows: 50_000, memory_bytes: 512 << 20 };
}

/// The first function of [`REFUSED_FUNCTIONS`] that `sql` calls (its name
/// followed by `(`, in any case, quoted or not, with comments between).
pub fn refused_call(sql: &str) -> Option<&'static str> {
    let tokens = Tokenizer::new(&GenericDialect {}, sql).tokenize().ok()?;
    let mut last_word: Option<&'static str> = None;
    for t in &tokens {
        match t {
            Token::Whitespace(_) => continue,
            Token::LParen if last_word.is_some() => return last_word,
            Token::Word(w) => {
                last_word = REFUSED_FUNCTIONS.iter().copied().find(|f| w.value.eq_ignore_ascii_case(f));
                continue;
            }
            _ => {}
        }
        last_word = None;
    }
    None
}

/// Whether `sql` is one read-only statement a caller may run: a single
/// `SELECT` (or `WITH ... SELECT`, or a set operation of them) that calls
/// none of [`REFUSED_FUNCTIONS`]. `Err` says why not.
pub fn check_select(sql: &str) -> Result<()> {
    let statements = chloedb::query::parse::parse(sql)?;
    match statements.as_slice() {
        [Statement::Query(_)] => {}
        [] => return Err(err("no statement")),
        [_] => return Err(err("only SELECT statements are allowed")),
        _ => return Err(err("one statement at a time: only a single SELECT is allowed")),
    }
    if let Some(f) = refused_call(sql) {
        return Err(err(format!("{f}() is not allowed here: it sends the rows it reads to a model")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_select_and_no_model_calls() {
        assert!(check_select("SELECT id, text FROM memory WHERE kind = 'decision' LIMIT 5").is_ok());
        assert!(check_select("  select count(*) from memory").is_ok());
        assert!(check_select("WITH t AS (SELECT id FROM memory LIMIT 3) SELECT id FROM t").is_ok());
        assert!(check_select("SELECT 'llm(' AS s, llm_count FROM memory LIMIT 1").is_ok(), "a string or a column is no call");
        for bad in [
            "DELETE FROM memory",
            "selectx",
            "SELECT 1; DELETE FROM memory",
            "SELECT 1; SELECT 2",
            "INSERT INTO memory (id) SELECT id FROM memory",
            "SELECT llm('openai/gpt-5-mini', 'repeat', text) FROM memory LIMIT 100",
            "SELECT LLM ('anthropic/claude-haiku-5-5', 'x', text) FROM memory",
            "SELECT \"llm\"('x', 'y', text) FROM memory",
            "SELECT llm/* c */('x', 'y', text) FROM memory",
            "SELECT id FROM memory ORDER BY cross_encoder('cohere/rerank-v3.5', text, 'q') DESC LIMIT 5",
            "",
        ] {
            assert!(check_select(bad).is_err(), "allowed: {bad}");
        }
    }
}
