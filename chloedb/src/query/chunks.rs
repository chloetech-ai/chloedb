//! Chunks of a table's text: `chunks_of(table, field, max_tokens => n,
//! overlap => m)`, a table of the pieces of each row's text, for a table
//! of chunks that a vector field embeds and a token-budgeted context packs.
//!
//! ```sql
//! SELECT id, n, tokens, chunk FROM chunks_of(docs, body, max_tokens => 256, overlap => 32)
//!   WHERE lang = 'en' LIMIT 20;
//!
//! INSERT INTO chunks (id, doc, n, body, tokens)
//!   SELECT chunk_id, id, n, chunk, tokens FROM chunks_of(docs, body, max_tokens => 256);
//! ```
//!
//! * **Splits.** A text is cut where it says most: before a Markdown
//!   heading, then at blank lines (paragraphs), then after a sentence's
//!   end, then between words, and between characters only for a word
//!   longer than a chunk. Pieces are then joined, in order, while a chunk
//!   holds at most `max_tokens` (256; from 16 to 8192), and a heading
//!   always starts a chunk.
//! * **Overlap.** Each chunk after the first starts with about `overlap`
//!   tokens (0 by default) of the end of the one before, from a word's
//!   start: `max_tokens` counts the chunk's own text, the overlap comes in
//!   front of it.
//! * **Tokens** are estimated as a quarter of the characters, rounded up
//!   ([`estimate_tokens`]): close for English text and the tokenizers of
//!   most models; a chunk's `tokens` counts its text with the overlap.
//! * **Columns.** `id` (the row's key, `NULL` in a table without one), `n`
//!   (0, 1, ...), `chunk_id` (`'<id>#<n>'`, a key for a table of
//!   chunks), `chunk`, `tokens`, `start_char` and `end_char` (where the
//!   chunk's own text starts and ends in the row's, in characters).
//! * **The query.** Its columns are those, or `*`; its `WHERE` reads the
//!   source table's rows (any condition of them), and its `LIMIT` cuts the
//!   chunks. `INSERT INTO t (columns) SELECT ... FROM chunks_of(...)`
//!   writes the chunks to a table, as an `INSERT` of their values.

use sqlparser::ast::{self as sql, Expr, FunctionArg, FunctionArgExpr, SelectItem, SetExpr, TableFactor};

use super::exec::{ExecStats, ResultSet};
use super::Params;
use crate::error::{Error, Result};
use crate::schema::Schema;
use crate::value::Value;

/// The function's name.
pub const NAME: &str = "chunks_of";

/// The columns a call gives.
pub const COLUMNS: [&str; 7] = ["id", "n", "chunk_id", "chunk", "tokens", "start_char", "end_char"];

const USAGE: &str = "SELECT columns FROM chunks_of(table, text_field [, max_tokens => 256] [, overlap => 0]) [WHERE condition of the table's rows] [LIMIT n], its columns id, n, chunk_id, chunk, tokens, start_char and end_char";

/// Rows of text a call reads at most.
pub const MAX_SOURCE_ROWS: usize = 1_000_000;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// The tokens a text is estimated to be: a quarter of its characters,
/// rounded up.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// One chunk of a text.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub n: usize,
    pub text: String,
    /// Where its own text (after the overlap) starts and ends, in
    /// characters.
    pub start: usize,
    pub end: usize,
    pub tokens: usize,
}

/// The pieces of `text[range]` at most `max` tokens each, cut at the first
/// level that cuts it (headings, paragraphs, sentences, words,
/// characters).
fn pieces(text: &str, range: std::ops::Range<usize>, level: usize, max: usize, out: &mut Vec<std::ops::Range<usize>>) {
    let part = &text[range.clone()];
    if estimate_tokens(part) <= max || part.trim().is_empty() {
        out.push(range);
        return;
    }
    let cuts: Vec<usize> = match level {
        // Before a line that starts a Markdown heading.
        0 => part.match_indices('\n').map(|(i, _)| i + 1).filter(|&i| part[i..].starts_with('#')).collect(),
        // After a blank line.
        1 => {
            let mut cs = Vec::new();
            let b = part.as_bytes();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'\n' {
                    let mut j = i + 1;
                    while j < b.len() && (b[j] == b' ' || b[j] == b'\t' || b[j] == b'\r') {
                        j += 1;
                    }
                    if j < b.len() && b[j] == b'\n' {
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        cs.push(j);
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
            cs
        }
        // After a sentence's end and its spaces.
        2 => {
            let mut cs = Vec::new();
            let mut it = part.char_indices().peekable();
            while let Some((i, c)) = it.next() {
                if matches!(c, '.' | '!' | '?' | '。' | '！' | '？') {
                    let mut j = i + c.len_utf8();
                    let mut spaced = false;
                    while let Some(&(k, d)) = it.peek() {
                        if d.is_whitespace() {
                            spaced = true;
                            j = k + d.len_utf8();
                            it.next();
                        } else {
                            break;
                        }
                    }
                    if spaced || c != '.' {
                        cs.push(j);
                    }
                }
            }
            cs
        }
        // Before a word.
        3 => part.char_indices().zip(part.chars().skip(1)).filter(|((_, c), d)| c.is_whitespace() && !d.is_whitespace()).map(|((i, c), _)| i + c.len_utf8()).collect(),
        // Between characters, a chunk's worth at a time.
        _ => part.char_indices().map(|(i, _)| i).enumerate().filter(|(k, _)| *k > 0 && k % (max * 4) == 0).map(|(_, i)| i).collect(),
    };
    let cuts: Vec<usize> = cuts.into_iter().filter(|&c| c > 0 && c < part.len()).collect();
    if cuts.is_empty() {
        pieces(text, range, level + 1, max, out);
        return;
    }
    let mut at = range.start;
    for c in cuts.into_iter().map(|c| range.start + c).chain(std::iter::once(range.end)) {
        if c > at {
            pieces(text, at..c, level + 1, max, out);
            at = c;
        }
    }
}

/// The chunks of `text`: at most `max_tokens` tokens of its own text each,
/// after `overlap` tokens of the chunk before ([module docs](self)).
pub fn split(text: &str, max_tokens: usize, overlap: usize) -> Vec<Chunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let max = max_tokens.max(1);
    let mut ps = Vec::new();
    pieces(text, 0..text.len(), 0, max, &mut ps);
    // Pieces joined while they fit, and a heading starts a chunk.
    let heading = |at: usize| (at == 0 || text[..at].ends_with('\n')) && text[at..].starts_with('#');
    let mut spans: Vec<std::ops::Range<usize>> = Vec::new();
    for p in ps {
        match spans.last_mut() {
            Some(s) if !heading(p.start) && estimate_tokens(&text[s.start..p.end]) <= max => s.end = p.end,
            _ => spans.push(p),
        }
    }
    // Characters before a byte, counted on from the last one asked for:
    // the spans are in order, so the whole text is counted once.
    let mut counted = (0usize, 0usize);
    let mut char_at = |byte: usize| {
        if byte < counted.0 {
            counted = (0, 0);
        }
        counted = (byte, counted.1 + text[counted.0..byte].chars().count());
        counted.1
    };
    let mut out = Vec::with_capacity(spans.len());
    for (n, s) in spans.into_iter().enumerate() {
        // Its own text, trimmed.
        let own = &text[s.clone()];
        let lead = own.len() - own.trim_start().len();
        let (start, end) = (s.start + lead, s.start + own.trim_end().len());
        if start >= end {
            continue;
        }
        // The overlap: from a word's start about `overlap` tokens before.
        let mut from = start;
        if overlap > 0 && n > 0 {
            let back = text[..start].char_indices().rev().nth(overlap * 4 - 1).map_or(0, |(i, _)| i);
            from = text[back..start].char_indices().find(|&(i, c)| i > 0 && !c.is_whitespace() && text[back..start][..i].ends_with(char::is_whitespace)).map_or(start, |(i, _)| back + i);
        }
        let body = text[from..end].to_string();
        let (start, end) = (char_at(start), char_at(end));
        out.push(Chunk { n: out.len(), tokens: estimate_tokens(&body), text: body, start, end });
    }
    out
}

/// A call of [`NAME`] in a query: what it reads, and what the query takes
/// of its rows.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunksQuery {
    pub table: String,
    pub field: String,
    pub max_tokens: usize,
    pub overlap: usize,
    /// The columns, in order: names of [`COLUMNS`], each with the name it
    /// answers as.
    pub columns: Vec<(String, String)>,
    /// The `WHERE` of the query, a condition of the table's rows.
    pub condition: Option<String>,
    pub limit: Option<usize>,
}

/// An `INSERT INTO table (columns) SELECT ... FROM chunks_of(...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunksInsert {
    pub table: String,
    pub columns: Vec<String>,
    pub query: ChunksQuery,
}

/// Whether text `sql` may call [`NAME`], a test that costs nothing.
pub fn may_call(sql: &str) -> bool {
    sql.as_bytes().windows(NAME.len()).any(|w| w.eq_ignore_ascii_case(NAME.as_bytes()))
}

fn value_arg(e: &Expr, params: &Params, what: &str) -> Result<usize> {
    match super::compound::value_of(e, params)? {
        Value::Int(n) if n >= 0 => Ok(n as usize),
        v => Err(err(format!("{NAME}(): {what} is a whole number, not {}", super::hint::sql_value(&v)))),
    }
}

fn name_of(e: &Expr) -> Option<String> {
    match e {
        Expr::Identifier(id) => Some(id.value.clone()),
        Expr::CompoundIdentifier(ids) => ids.last().map(|i| i.value.clone()),
        _ => None,
    }
}

/// The call of [`NAME`] query `q` is, if it reads one: its only `FROM`,
/// without joins, `GROUP BY`, `ORDER BY` or `OFFSET`.
pub fn of_query(q: &sql::Query, params: &Params) -> Result<Option<ChunksQuery>> {
    let SetExpr::Select(s) = q.body.as_ref() else { return Ok(None) };
    let [from] = s.from.as_slice() else { return Ok(None) };
    let TableFactor::Table { name, args: Some(args), .. } = &from.relation else { return Ok(None) };
    if !name.to_string().eq_ignore_ascii_case(NAME) {
        return Ok(None);
    }
    if !from.joins.is_empty() || q.with.is_some() || q.order_by.is_some() || !matches!(s.group_by, sql::GroupByExpr::Expressions(ref g, _) if g.is_empty()) || s.having.is_some() || s.distinct.is_some() {
        return Err(err(format!("{NAME}() is read as it is: {USAGE}; to join, order or group its rows, write them to a table first (INSERT INTO chunks SELECT ... FROM {NAME}(...))")));
    }
    let mut positional: Vec<&Expr> = Vec::new();
    let (mut max_tokens, mut overlap) = (256usize, 0usize);
    for a in &args.args {
        match a {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => positional.push(e),
            FunctionArg::Named { name, arg: FunctionArgExpr::Expr(e), .. } | FunctionArg::ExprNamed { name: Expr::Identifier(name), arg: FunctionArgExpr::Expr(e), .. } => match name.value.to_ascii_lowercase().as_str() {
                "max_tokens" => max_tokens = value_arg(e, params, "max_tokens")?,
                "overlap" => overlap = value_arg(e, params, "overlap")?,
                n => return Err(err(format!("{NAME}() has no option {n}; {USAGE}"))),
            },
            _ => return Err(err(USAGE)),
        }
    }
    let [table, field] = positional.as_slice() else { return Err(err(format!("{NAME}() takes a table and its text field; {USAGE}"))) };
    let (Some(table), Some(field)) = (name_of(table), name_of(field)) else { return Err(err(format!("{NAME}(): the table and the field are names, not text; {USAGE}"))) };
    if !(16..=8192).contains(&max_tokens) {
        return Err(err(format!("{NAME}(): max_tokens is from 16 to 8192, not {max_tokens}")));
    }
    if overlap >= max_tokens {
        return Err(err(format!("{NAME}(): overlap is fewer tokens than max_tokens")));
    }
    let mut columns = Vec::new();
    for item in &s.projection {
        match item {
            SelectItem::Wildcard(_) => columns.extend(COLUMNS.iter().map(|c| (c.to_string(), c.to_string()))),
            SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => {
                let Some(c) = name_of(e).map(|c| c.to_ascii_lowercase()).filter(|c| COLUMNS.contains(&c.as_str())) else {
                    return Err(err(format!("{NAME}() gives the columns {}, not {e}", COLUMNS.join(", "))));
                };
                let alias = match item {
                    SelectItem::ExprWithAlias { alias, .. } => alias.value.clone(),
                    _ => c.clone(),
                };
                columns.push((c, alias));
            }
            _ => return Err(err(USAGE)),
        }
    }
    let (offset, limit) = super::compound::limits(&q.limit_clause, params)?;
    if offset > 0 {
        return Err(err(format!("{NAME}() takes a LIMIT and no OFFSET")));
    }
    Ok(Some(ChunksQuery { table, field, max_tokens, overlap, columns, condition: s.selection.as_ref().map(|w| w.to_string()), limit }))
}

/// [`of_query`] of the text of a query.
pub fn of_sql(sql_text: &str, params: &Params) -> Result<Option<ChunksQuery>> {
    if !may_call(sql_text) {
        return Ok(None);
    }
    let Ok(q) = super::sql::parse_query(sql_text) else { return Ok(None) };
    of_query(&q, params)
}

/// The `INSERT ... SELECT ... FROM chunks_of(...)` of text `sql_text`.
pub fn insert_of_sql(sql_text: &str, params: &Params) -> Result<Option<ChunksInsert>> {
    if !may_call(sql_text) || !sql_text.trim_start().get(..6).is_some_and(|w| w.eq_ignore_ascii_case("INSERT")) {
        return Ok(None);
    }
    let Ok(stmt) = super::sql::parse_one(sql_text) else { return Ok(None) };
    let sql::Statement::Insert(ins) = stmt else { return Ok(None) };
    let Some(src) = &ins.source else { return Ok(None) };
    let Some(query) = of_query(src, params)? else { return Ok(None) };
    if ins.on.is_some() || ins.returning.is_some() {
        return Err(err(format!("an INSERT of {NAME}()'s rows takes no ON CONFLICT or RETURNING")));
    }
    let columns: Vec<String> = ins.columns.iter().map(|c| c.value.clone()).collect();
    if columns.len() != query.columns.len() {
        return Err(err(format!("INSERT INTO {} names {} columns and its SELECT of {NAME}() gives {}", ins.table, columns.len(), query.columns.len())));
    }
    Ok(Some(ChunksInsert { table: ins.table.to_string(), columns, query }))
}

impl ChunksQuery {
    /// The query that reads the rows it splits: each row's key and text.
    ///
    /// It reads [`ChunksQuery::source_rows`] rows: as many as the query's
    /// LIMIT, since every row with text gives a chunk at least. When rows
    /// without text leave the LIMIT unfilled, [`ChunksQuery::more_rows`]
    /// says how many to read again.
    pub fn source_sql(&self, schema: &Schema) -> Result<String> {
        self.source_sql_of(schema, self.source_rows())
    }

    /// The rows [`ChunksQuery::source_sql`] reads first: the query's
    /// LIMIT, or [`MAX_SOURCE_ROWS`] without one.
    pub fn source_rows(&self) -> usize {
        self.limit.map_or(MAX_SOURCE_ROWS, |l| l.max(1)).min(MAX_SOURCE_ROWS)
    }

    /// After a read of `read` rows that gave `rs` and the chunks `chunks`
    /// ([`ChunksQuery::finish`]): how many rows to read instead, when the
    /// LIMIT is not filled and the table may hold more rows (twice as
    /// many, up to [`MAX_SOURCE_ROWS`]); `None` when the answer is whole.
    pub fn more_rows(&self, read: usize, rs: &ResultSet, chunks: &ResultSet) -> Option<usize> {
        let limit = self.limit?;
        (chunks.rows.len() < limit && rs.rows.len() >= read && read < MAX_SOURCE_ROWS).then(|| read.saturating_mul(2).min(MAX_SOURCE_ROWS))
    }

    /// [`ChunksQuery::source_sql`] reading `rows` rows at most.
    pub fn source_sql_of(&self, schema: &Schema, rows: usize) -> Result<String> {
        let f = schema.field(&self.field).ok_or_else(|| err(format!("{NAME}(): {} has no field {}", schema.table, self.field)))?;
        if !f.stored || !matches!(f.ty, crate::schema::FieldType::Text { .. } | crate::schema::FieldType::Keyword) || f.multi {
            return Err(err(format!("{NAME}(): {} is not a stored TEXT field", f.name)));
        }
        let key = schema.primary_key.map(|k| super::sql::ident(&schema.fields[k].name)).unwrap_or_else(|| "NULL".into());
        let cond = self.condition.as_ref().map_or(String::new(), |c| format!(" WHERE {c}"));
        Ok(format!("SELECT {key}, {} FROM {}{cond} LIMIT {}", super::sql::ident(&f.name), super::sql::ident(&schema.table), rows.clamp(1, MAX_SOURCE_ROWS)))
    }

    /// The query's answer, its rows read by `read` (a query of the
    /// table's rows, as [`ChunksQuery::source_sql_of`] writes it): as few
    /// rows as fill the LIMIT.
    pub fn run(&self, schema: &Schema, mut read: impl FnMut(&str) -> Result<ResultSet>) -> Result<ResultSet> {
        let mut rows = self.source_rows();
        loop {
            let rs = read(&self.source_sql_of(schema, rows)?)?;
            let out = self.finish(&rs);
            match self.more_rows(rows, &rs, &out) {
                Some(more) => rows = more,
                None => return Ok(out),
            }
        }
    }

    /// The chunks of the rows `rs` (of [`ChunksQuery::source_sql`]) as
    /// the query's rows.
    pub fn finish(&self, rs: &ResultSet) -> ResultSet {
        let mut rows = Vec::new();
        'rows: for row in &rs.rows {
            let (key, text) = (row.first().cloned().unwrap_or(Value::Null), row.get(1).and_then(|t| t.as_str()).unwrap_or(""));
            for c in split(text, self.max_tokens, self.overlap) {
                if self.limit.is_some_and(|l| rows.len() >= l) {
                    break 'rows;
                }
                let id = match &key {
                    Value::Null => String::new(),
                    Value::Str(s) => s.clone(),
                    v => super::hint::sql_value(v).trim_matches('\'').to_string(),
                };
                rows.push(
                    self.columns
                        .iter()
                        .map(|(col, _)| match col.as_str() {
                            "id" => key.clone(),
                            "n" => Value::Int(c.n as i64),
                            "chunk_id" => Value::Str(format!("{id}#{}", c.n)),
                            "chunk" => Value::Str(c.text.clone()),
                            "tokens" => Value::Int(c.tokens as i64),
                            "start_char" => Value::Int(c.start as i64),
                            _ => Value::Int(c.end as i64),
                        })
                        .collect::<Vec<Value>>(),
                );
            }
        }
        let n = rows.len();
        let mut stats = ExecStats::default();
        stats.plan.push(format!("chunks_of: {} rows of {}.{} in {n} chunks of {} tokens at most", rs.rows.len(), self.table, self.field, self.max_tokens));
        ResultSet { columns: self.columns.iter().map(|(_, a)| a.clone()).collect(), rows, keys: vec![Value::Null; n], scores: vec![0.0; n], parts: None, stats }
    }
}

impl ChunksInsert {
    /// The `INSERT` statements of `rows` (of [`ChunksQuery::finish`]), a
    /// few hundred rows each, and their parameters.
    pub fn inserts(&self, rows: &ResultSet) -> Vec<(String, Params)> {
        let cols = self.columns.iter().map(|c| super::sql::ident(c)).collect::<Vec<_>>().join(", ");
        rows.rows
            .chunks(500)
            .map(|batch| {
                let mut p = Params::new();
                let mut values = Vec::with_capacity(batch.len());
                for (r, row) in batch.iter().enumerate() {
                    let names: Vec<String> = (0..row.len()).map(|c| format!(":c{r}_{c}")).collect();
                    for (c, v) in row.iter().enumerate() {
                        p = p.set(&format!("c{r}_{c}"), v.clone());
                    }
                    values.push(format!("({})", names.join(", ")));
                }
                (format!("INSERT INTO {} ({cols}) VALUES {}", self.table, values.join(", ")), p)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_headings_then_paragraphs_then_sentences() {
        let text = "# Intro\nShort start.\n\n# Methods\nWe did one thing. Then we did another thing that took a while.\n\nAnd a second paragraph here.";
        let cs = split(text, 16, 0);
        assert!(cs.iter().all(|c| c.tokens <= 16), "{cs:?}");
        assert_eq!(cs[0].text, "# Intro\nShort start.", "a heading's section first: {cs:?}");
        assert!(cs[1].text.starts_with("# Methods"), "{cs:?}");
        // Every character of the text is in a chunk, in order.
        let joined: String = cs.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("");
        assert_eq!(joined.split_whitespace().collect::<String>(), text.split_whitespace().collect::<String>());
        for c in &cs {
            assert_eq!(text.chars().skip(c.start).take(c.end - c.start).collect::<String>(), c.text, "offsets of {c:?}");
        }
        // A word longer than a chunk is cut between characters.
        let long = "x".repeat(200);
        assert!(split(&long, 16, 0).iter().all(|c| c.tokens <= 16));
        assert!(split("  \n ", 16, 0).is_empty());
    }

    #[test]
    fn overlap_repeats_the_end_of_the_chunk_before() {
        let text = (0..200).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
        let cs = split(&text, 32, 8);
        assert!(cs.len() > 3);
        for w in cs.windows(2) {
            let tail: Vec<&str> = w[0].text.split_whitespace().rev().take(3).collect();
            assert!(tail.iter().all(|t| w[1].text.contains(t)), "{:?} then {:?}", w[0].text, w[1].text);
            assert!(w[1].text.starts_with("word"), "from a word's start: {:?}", w[1].text);
        }
    }
}
