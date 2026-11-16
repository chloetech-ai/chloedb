//! Search: a search box's words, understood (`query`), as SQL over the
//! stories and the comments. Every statement it runs is kept in the
//! request's trace, so the page can show the SQL behind each answer.
//!
//! Ranking a story, "best" (the default):
//!
//! 1. Candidates by words: BM25 over a story's title, link words and text,
//!    typos corrected (`fuzzy => 'auto'`) and HN's synonyms (`k8s`,
//!    `postgres`) widened, times a popularity prior (`pop`, ln(1+points)),
//!    a rank-safe boost: the exact top rows by the product, not the best of
//!    BM25's top 100.
//! 2. Candidates by meaning: the nearest titles to the query's vector.
//! 3. Both reranked together: the title's BM25 counts most, a phrase of the
//!    whole query in the title more, then the text, the meaning, points
//!    and a gentle preference for the recent.

use serde_json::{json, Value};

use crate::query::{self, Parsed};
use crate::strata::{Row, RowExt, Strata, Trace};

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub q: String,
    /// `all`, `story` or `comment`.
    pub only: String,
    /// `best`, `date`, `points`, `comments`.
    pub sort: String,
    /// `hybrid`, `words` or `meaning`.
    pub mode: String,
    pub page: usize,
    pub per_page: usize,
    pub facets: bool,
}

pub struct Ctx<'a> {
    pub db: &'a Strata,
    pub trace: &'a Trace,
}

impl<'a> Ctx<'a> {
    /// A query's rows, kept in the trace. The texts are copied before the
    /// future starts, so a `&format!(..)` may be passed inside a `join!`.
    pub fn rows(&self, what: &str, sql: &str, params: Value) -> impl std::future::Future<Output = Result<Vec<Row>, crate::strata::Error>> + 'a {
        let (db, trace, what, sql) = (self.db, self.trace, what.to_string(), sql.to_string());
        async move { db.traced(trace, &what, &sql, params).await }
    }
    /// The same, with no rows for a query refused (the trace says why).
    pub fn soft(&self, what: &str, sql: &str, params: Value) -> impl std::future::Future<Output = Vec<Row>> + 'a {
        let f = self.rows(what, sql, params);
        async move { f.await.unwrap_or_default() }
    }
}

pub const STORY_COLS: &str = "id, title, url, site, domain, author, created, points, comments, kind, topics";
pub const COMMENT_COLS: &str = "id, body, author, created, story, story_title, parent, parent_author, depth, replies, thread";

fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

/// The filters a query's understanding gives, as SQL with parameters, for
/// the stories (`story`) or the comments.
pub fn filters(p: &Parsed, story_table: bool, params: &mut serde_json::Map<String, Value>) -> Vec<String> {
    let mut w = Vec::new();
    if let Some(a) = &p.author {
        params.insert("author".into(), json!(a));
        w.push("author = :author".to_string());
    }
    if let Some(a) = p.after {
        w.push(format!("created >= TIMESTAMP '{}'", iso(a)));
    }
    if let Some(b) = p.before {
        w.push(format!("created < TIMESTAMP '{}'", iso(b)));
    }
    // Text in backticks, exactly: the substring index (an FM-index a
    // segment) answers `ILIKE` without reading the rows.
    for (i, x) in p.exact.iter().enumerate() {
        params.insert(format!("ex{i}"), json!(like(x)));
        w.push(if story_table { format!("(title ILIKE :ex{i} OR url ILIKE :ex{i})") } else { format!("body ILIKE :ex{i}") });
    }
    if story_table {
        if !p.kinds.is_empty() {
            let ks: Vec<String> = p.kinds.iter().map(|k| format!("'{}'", k.replace('\'', ""))).collect();
            w.push(format!("kind IN ({})", ks.join(", ")));
        }
        if let Some(s) = &p.site {
            params.insert("site".into(), json!(s));
            w.push(if s.contains('/') { "site = :site".into() } else { "(domain = :site OR site = :site)".into() });
        }
        if let Some(t) = &p.topic {
            params.insert("topic".into(), json!(t));
            w.push("topics = :topic".into());
        }
        if let Some(n) = p.points_min {
            w.push(format!("points >= {n}"));
        }
        if let Some(n) = p.points_max {
            w.push(format!("points <= {n}"));
        }
        if let Some(n) = p.comments_min {
            w.push(format!("comments >= {n}"));
        }
    } else if let Some(s) = p.story {
        w.push(format!("story = {s}"));
    }
    w
}

/// A `%text%` pattern for `ILIKE` (a `%` in the text matches any one
/// character: the substring index takes no `ESCAPE`).
pub fn like(x: &str) -> String {
    format!("%{}%", x.replace('%', "_"))
}

fn and(conds: &[String]) -> String {
    if conds.is_empty() {
        String::new()
    } else {
        conds.join(" AND ")
    }
}

/// How many of a query's words a row must hold: all of up to three (as
/// hn.algolia.com asks), three of four, 60% of more. Longer queries are
/// sentences, where meaning does the work.
pub fn min_words(text: &str) -> String {
    let n = text.split_whitespace().count();
    match n {
        0..=3 => format!("{}", n.max(1)),
        4 => "3".into(),
        _ => "60%".into(),
    }
}

/// A story the query's words are in: its title, its text or its link's
/// words, each holding `:min` of the words `:m` (synonyms counted). Typos
/// are not: `fuzzy` here would count "trust" and "rest" as "rust" (a
/// count of 81,330 titles for 21,243); a query whose words match nothing
/// is counted by [`STORY_MATCH_FUZZY`].
pub const STORY_MATCH: &str = "(MATCH(title, :m, synonyms => 1, min => :min) OR MATCH(body, :m, synonyms => 1, min => :min) OR MATCH(urlwords, :m, min => :min))";

/// [`STORY_MATCH`] with typos, for a query whose words match nothing as
/// typed.
pub const STORY_MATCH_FUZZY: &str = "(MATCH(title, :m, fuzzy => 'auto', synonyms => 1, min => :min) OR MATCH(body, :m, fuzzy => 'auto', synonyms => 1, min => :min) OR MATCH(urlwords, :m, fuzzy => 'auto', min => :min))";

/// A comment the query's words are in (see [`STORY_MATCH`]).
pub const COMMENT_MATCH: &str = "MATCH(body, :m, synonyms => 1, min => :min)";
pub const COMMENT_MATCH_FUZZY: &str = "MATCH(body, :m, fuzzy => 'auto', synonyms => 1, min => :min)";

/// The words a query leaves out (`-word`), one condition each, on the
/// field given.
pub fn excluded(p: &Parsed, field: &str, params: &mut serde_json::Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    for (i, w) in p.exclude.iter().enumerate() {
        let w: String = w.chars().filter(|c| c.is_alphanumeric() || matches!(c, '+' | '#' | '.' | '\'')).collect();
        if w.is_empty() {
            continue;
        }
        params.insert(format!("x{i}"), json!(w));
        out.push(format!("NOT MATCH({field}, :x{i})"));
    }
    out
}

pub struct Found {
    pub stories: Vec<Row>,
    pub comments: Vec<Row>,
    pub total_stories: i64,
    pub total_comments: i64,
}

/// The stories for a query.
pub async fn stories(c: &Ctx<'_>, p: &Parsed, r: &Request, limit: usize, offset: usize) -> Result<(Vec<Row>, i64), crate::strata::Error> {
    let mut params = serde_json::Map::new();
    let mut conds = filters(p, true, &mut params);
    let words = query::plain_words(p);
    let m = query::match_text(p);
    conds.extend(excluded(p, "title", &mut params));
    let has_text = !words.is_empty();
    if has_text {
        params.insert("q".into(), json!(words));
        params.insert("m".into(), json!(m));
        params.insert("min".into(), json!(min_words(&words)));
    }
    let mode = if r.mode.is_empty() { "hybrid" } else { r.mode.as_str() };
    let wh = and(&conds);
    let where_ = |extra: &str| -> String {
        let all: Vec<&str> = [wh.as_str(), extra].into_iter().filter(|s| !s.is_empty()).collect();
        if all.is_empty() { String::new() } else { format!("WHERE {}", all.join(" AND ")) }
    };
    let order_simple = match r.sort.as_str() {
        "date" => Some("created DESC"),
        "old" => Some("created ASC"),
        "points" => Some("points DESC"),
        "comments" => Some("comments DESC"),
        "least_points" => Some("points ASC"),
        "least_comments" => Some("comments ASC"),
        _ => None,
    };
    if !has_text {
        let ord = order_simple.unwrap_or("points DESC");
        let sql = format!("SELECT {STORY_COLS} FROM hn_stories {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_(""));
        let count_where = where_("");
        let (rows, total) = tokio::join!(c.rows("stories", &sql, Value::Object(params.clone())), count(c, "hn_stories", &count_where, &params));
        let rows = rows?;
        return Ok((rows, total));
    }
    // Words must be there (all of a short query); typos and synonyms widen them.
    let matched = STORY_MATCH;
    if let Some(ord) = order_simple {
        let sql = format!("SELECT {STORY_COLS} FROM hn_stories {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_(matched));
        let count_where = where_(matched);
        let (rows, total) = tokio::join!(c.rows("stories", &sql, Value::Object(params.clone())), count(c, "hn_stories", &count_where, &params));
        let rows = rows?;
        return Ok((rows, total));
    }
    if mode == "exact" {
        // The words exactly as typed, inside words and punctuation too.
        params.insert("xall".into(), json!(like(&words)));
        let cond = "(title ILIKE :xall OR url ILIKE :xall)";
        let ord = order_simple.unwrap_or("points DESC");
        let sql = format!("SELECT {STORY_COLS} FROM hn_stories {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_(cond));
        let count_where = where_(cond);
        let (rows, total) = tokio::join!(c.rows("stories, the text exactly (substring index)", &sql, Value::Object(params.clone())), count(c, "hn_stories", &count_where, &params));
        let rows = rows?;
        return Ok((rows, total));
    }
    if mode == "meaning" {
        params.insert("v".into(), json!({"embed": words, "field": "emb", "table": "hn_stories"}));
        let sql = format!("SELECT {STORY_COLS}, score() AS score FROM hn_stories {} ORDER BY similarity(emb, :v) DESC LIMIT {limit} OFFSET {offset}", where_(""));
        let rows = c.rows("stories by meaning", &sql, Value::Object(params.clone())).await?;
        return Ok((rows, 0));
    }
    // Phase 1, words: the exact top rows by BM25 times popularity.
    let cand = (offset + limit).max(150) * 2;
    let lex = format!(
        "SELECT id FROM hn_stories {} ORDER BY bm25(title, :q, fuzzy => 'auto', synonyms => 1, min => :min) * (1 + 0.35 * pop) DESC LIMIT {cand}",
        where_("")
    );
    let body_sql = format!(
        "SELECT id FROM hn_stories {} ORDER BY bm25(body, :q, fuzzy => 'auto', synonyms => 1, min => :min) * (1 + 0.35 * pop) DESC LIMIT 60",
        where_("")
    );
    // Code as written (`std::move`, `node.js`): the substring index finds
    // what the analyzer split or dropped.
    let mut code_sql = Vec::new();
    for (i, x) in p.code.iter().take(2).enumerate() {
        params.insert(format!("cx{i}"), json!(like(x)));
        code_sql.push((format!("candidates with “{x}” exactly (substring index)"), format!("SELECT id FROM hn_stories {} ORDER BY points DESC LIMIT 100", where_(&format!("(title ILIKE :cx{i} OR url ILIKE :cx{i})")))));
    }
    if mode == "hybrid" {
        params.insert("v".into(), json!({"embed": words, "field": "emb", "table": "hn_stories"}));
    }
    let vec_sql = format!("SELECT id FROM hn_stories {} ORDER BY similarity(emb, :v) * (1 + 0.1 * pop) DESC LIMIT 80", where_(""));
    // Every candidate list and the count at once: one round trip's time, not five.
    let pv = Value::Object(params.clone());
    let count_where = where_(matched);
    let (lex_rows, body_rows, code_rows, v, total) = tokio::join!(
        c.rows("candidates by words", &lex, pv.clone()),
        c.soft("candidates by text", &body_sql, pv.clone()),
        futures_util::future::join_all(code_sql.iter().map(|(what, sql)| c.soft(what, sql, pv.clone()))),
        async { if mode == "hybrid" { c.soft("candidates by meaning", &vec_sql, pv.clone()).await } else { Vec::new() } },
        count(c, "hn_stories", &count_where, &params),
    );
    let lex_rows = lex_rows?;
    let mut ids: Vec<i64> = lex_rows.iter().chain(body_rows.iter()).chain(code_rows.iter().flatten()).chain(v.iter()).map(|r| r.i("id")).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let list: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    params.insert("phrase".into(), json!(format!("\"{}\"", words.replace('"', ""))));
    let sim = if mode == "hybrid" { " + 2.5 * coalesce(similarity(emb, :v), 0)" } else { "" };
    // Phase 2: the candidates, ranked together: how well the words fit
    // (title first, the words together, then the text and the link), times
    // the votes, strongly, as HN readers expect of a search of HN (a 700
    // point story about the words beats a 60 point one with a shorter
    // title), a little newer first.
    let rank = format!(
        "(coalesce(bm25(title, :q, fuzzy => 'auto', synonyms => 1), 0) + 0.8 * coalesce(bm25(title, :phrase), 0) + 0.2 * coalesce(bm25(body, :q, fuzzy => 'auto', synonyms => 1), 0) + 0.3 * coalesce(bm25(urlwords, :q), 0){sim}) * pow(points + 1, 0.35) * (0.8 + 0.2 * decay(created, now(), 157680000))"
    );
    let sql = format!("SELECT {STORY_COLS}, score() AS score FROM hn_stories WHERE id IN ({}) ORDER BY {rank} DESC LIMIT {limit} OFFSET {offset}", list.join(", "));
    let rows = c.rows("stories, ranked", &sql, Value::Object(params.clone())).await?;
    let mut total = total;
    if total == 0 && !rows.is_empty() {
        total = count(c, "hn_stories", &where_(STORY_MATCH_FUZZY), &params).await;
    }
    Ok((rows, total))
}

/// A title without its "Show HN: " (the page shows the kind beside it).
pub fn short_title(t: &str) -> &str {
    for p in ["Show HN:", "Ask HN:", "Launch HN:", "Tell HN:"] {
        if let Some(head) = t.get(..p.len()).filter(|h| h.eq_ignore_ascii_case(p) && t.len() > p.len()) {
            return t[head.len()..].trim_start();
        }
    }
    t
}

/// The texts with what the query matches marked by the server
/// (`POST /v1/highlight`: stems, synonyms, typos and code as the field's
/// analyzer reads them), HTML-escaped between `<mark>` tags; `None` from a
/// server without it.
pub async fn highlight(c: &Ctx<'_>, table: &str, field: &str, query: &str, texts: Vec<String>, size: usize) -> Option<Vec<String>> {
    if texts.is_empty() || query.trim().is_empty() {
        return None;
    }
    let t0 = std::time::Instant::now();
    let n = texts.len();
    let r = c.db.post("/v1/highlight", &json!({"table": table, "field": field, "query": query, "texts": texts, "size": size})).await;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let shown = format!("POST /v1/highlight {{table: {table}, field: {field}, query: {query:?}, size: {size}, texts: {n}}}");
    match r {
        Ok(j) => {
            c.trace.push(&format!("highlights of {table}.{field}"), &shown, &Value::Null, n, ms, None);
            j["texts"].as_array().map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect())
        }
        Err(e) => {
            c.trace.push(&format!("highlights of {table}.{field}"), &shown, &Value::Null, 0, ms, Some(&format!("refused: {}", e.message)));
            None
        }
    }
}

/// Marks the stories' titles and texts, and the comments' texts, for the
/// query.
pub async fn mark(c: &Ctx<'_>, p: &Parsed, stories: &mut [Row], comments: &mut [Row]) {
    let q = query::match_text(p);
    if q.trim().is_empty() {
        return;
    }
    let titles: Vec<String> = stories.iter().map(|r| short_title(&r.s("title")).to_string()).collect();
    let bodies: Vec<String> = stories.iter().map(|r| r.s("body")).collect();
    let cbodies: Vec<String> = comments.iter().map(|r| r.s("body")).collect();
    let has_body = bodies.iter().any(|b| !b.is_empty());
    let (t, b, cb) = tokio::join!(
        highlight(c, "hn_stories", "title", &q, titles, 0),
        async { if has_body { highlight(c, "hn_stories", "body", &q, bodies, 240).await } else { None } },
        highlight(c, "hn_comments", "body", &q, cbodies, 420),
    );
    for (i, r) in stories.iter_mut().enumerate() {
        if let Some(x) = t.as_ref().and_then(|v| v.get(i)) {
            r.insert("title_hl".into(), json!(x));
        }
        if let Some(x) = b.as_ref().and_then(|v| v.get(i)).filter(|x| !x.is_empty()) {
            r.insert("body_hl".into(), json!(x));
        }
    }
    for (i, r) in comments.iter_mut().enumerate() {
        if let Some(x) = cb.as_ref().and_then(|v| v.get(i)) {
            r.insert("body_hl".into(), json!(x));
        }
    }
}

async fn count(c: &Ctx<'_>, table: &str, wh: &str, params: &serde_json::Map<String, Value>) -> i64 {
    let sql = format!("SELECT COUNT(*) AS n FROM {table} {wh}");
    c.soft("how many match", &sql, Value::Object(params.clone())).await.first().map(|r| r.i("n")).unwrap_or(0)
}

/// The comments for a query: BM25 over their text, the story's title
/// counted too (a comment in a thread about the words is about them more
/// likely), HN's own order and the discussion under a comment as its prior,
/// at most two of a thread.
pub async fn comments(c: &Ctx<'_>, p: &Parsed, r: &Request, limit: usize, offset: usize) -> Result<(Vec<Row>, i64), crate::strata::Error> {
    let mut params = serde_json::Map::new();
    let mut conds = filters(p, false, &mut params);
    let words = query::plain_words(p);
    let m = query::match_text(p);
    conds.extend(excluded(p, "body", &mut params));
    let wh = and(&conds);
    let where_ = |extra: &str| -> String {
        let all: Vec<&str> = [wh.as_str(), extra].into_iter().filter(|s| !s.is_empty()).collect();
        if all.is_empty() { String::new() } else { format!("WHERE {}", all.join(" AND ")) }
    };
    if words.is_empty() {
        let ord = match r.sort.as_str() { "comments" => "replies DESC, thread DESC", "points" => "pop DESC", "old" => "created ASC", "least_comments" => "replies ASC", _ => "created DESC" };
        let sql = format!("SELECT {COMMENT_COLS} FROM hn_comments {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_(""));
        let count_where = where_("");
        let (rows, total) = tokio::join!(c.rows("comments", &sql, Value::Object(params.clone())), count(c, "hn_comments", &count_where, &params));
        let rows = rows?;
        return Ok((rows, total));
    }
    if r.mode == "exact" {
        params.insert("xall".into(), json!(like(&words)));
        let ord = match r.sort.as_str() { "comments" => "replies DESC", "old" => "created ASC", _ => "created DESC" };
        let sql = format!("SELECT {COMMENT_COLS} FROM hn_comments {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_("body ILIKE :xall"));
        let count_where = where_("body ILIKE :xall");
        let (rows, total) = tokio::join!(c.rows("comments, the text exactly (substring index)", &sql, Value::Object(params.clone())), count(c, "hn_comments", &count_where, &params));
        let rows = rows?;
        return Ok((rows, total));
    }
    params.insert("q".into(), json!(words));
    params.insert("m".into(), json!(m));
    params.insert("min".into(), json!(min_words(&words)));
    let matched = COMMENT_MATCH;
    let ord = match r.sort.as_str() {
        "date" => "created DESC".to_string(),
        "old" => "created ASC".to_string(),
        "comments" => "replies DESC, thread DESC".to_string(),
        "points" => "pop DESC".to_string(),
        _ => "bm25(body, :q, fuzzy => 'auto', synonyms => 1, min => :min) * (1 + 0.25 * pop) DESC".to_string(),
    };
    let per = if p.story.is_some() { "" } else { " MAX 2 PER story" };
    let sql = format!("SELECT {COMMENT_COLS}, score() AS score FROM hn_comments {} ORDER BY {ord}{per} LIMIT {limit} OFFSET {offset}", where_(if r.sort.is_empty() || r.sort == "best" { "" } else { matched }));
    let count_where = where_(matched);
    let (rows, mut total) = tokio::join!(c.rows("comments", &sql, Value::Object(params.clone())), count(c, "hn_comments", &count_where, &params));
    let rows = rows?;
    if total == 0 && !rows.is_empty() {
        total = count(c, "hn_comments", &where_(COMMENT_MATCH_FUZZY), &params).await;
    }
    Ok((rows, total))
}

/// Counts by kind, year, site, author, topic and points for the stories a
/// query matches: the facets beside the results.
pub async fn facets(c: &Ctx<'_>, p: &Parsed, only_comments: bool) -> Value {
    let mut params = serde_json::Map::new();
    let words = query::plain_words(p);
    let m = query::match_text(p);
    let mut out = serde_json::Map::new();
    if only_comments {
        let mut conds = filters(p, false, &mut params);
        conds.extend(excluded(p, "body", &mut params));
        if !words.is_empty() {
            params.insert("m".into(), json!(m));
            params.insert("min".into(), json!(min_words(&words)));
            conds.push(COMMENT_MATCH.into());
        }
        let wh = if conds.is_empty() { String::new() } else { format!("WHERE {}", conds.join(" AND ")) };
        let pr = Value::Object(params.clone());
        let (years, authors) = tokio::join!(
            c.soft("comments by year", &format!("SELECT year, COUNT(*) AS n FROM hn_comments {wh} GROUP BY year ORDER BY year"), pr.clone()),
            c.soft("comments by person", &format!("SELECT author, COUNT(*) AS n FROM hn_comments {wh} GROUP BY author ORDER BY n DESC LIMIT 12"), pr.clone()),
        );
        out.insert("year".into(), facet(&years, "year"));
        out.insert("author".into(), facet(&authors, "author"));
        return Value::Object(out);
    }
    let mut conds = filters(p, true, &mut params);
    conds.extend(excluded(p, "title", &mut params));
    if !words.is_empty() {
        params.insert("m".into(), json!(m));
        params.insert("min".into(), json!(min_words(&words)));
        conds.push(STORY_MATCH.into());
    }
    let wh = if conds.is_empty() { String::new() } else { format!("WHERE {}", conds.join(" AND ")) };
    let pr = Value::Object(params.clone());
    let points = "CASE WHEN points >= 1000 THEN '1000+' WHEN points >= 300 THEN '300+' WHEN points >= 100 THEN '100+' WHEN points >= 20 THEN '20+' ELSE '<20' END";
    let (kinds, years, sites, authors, topics, pts) = tokio::join!(
        c.soft("stories by kind", &format!("SELECT kind, COUNT(*) AS n FROM hn_stories {wh} GROUP BY kind ORDER BY n DESC"), pr.clone()),
        c.soft("stories by year", &format!("SELECT year, COUNT(*) AS n FROM hn_stories {wh} GROUP BY year ORDER BY year"), pr.clone()),
        c.soft("stories by site", &format!("SELECT site, COUNT(*) AS n FROM hn_stories {wh} GROUP BY site ORDER BY n DESC LIMIT 13"), pr.clone()),
        c.soft("stories by person", &format!("SELECT author, COUNT(*) AS n FROM hn_stories {wh} GROUP BY author ORDER BY n DESC LIMIT 12"), pr.clone()),
        c.soft("stories by topic", &format!("SELECT topics, COUNT(*) AS n FROM hn_stories {wh} GROUP BY topics ORDER BY n DESC LIMIT 14"), pr.clone()),
        c.soft("stories by points", &format!("SELECT {points} AS b, COUNT(*) AS n FROM hn_stories {wh} GROUP BY b"), pr.clone()),
    );
    out.insert("kind".into(), facet(&kinds, "kind"));
    out.insert("year".into(), facet(&years, "year"));
    out.insert("site".into(), json!(facet(&sites, "site").as_array().map(|a| a.iter().filter(|x| x["value"] != "" && !x["value"].is_null()).take(12).cloned().collect::<Vec<_>>()).unwrap_or_default()));
    out.insert("author".into(), facet(&authors, "author"));
    out.insert("topic".into(), facet(&topics, "topics"));
    out.insert("points".into(), facet(&pts, "b"));
    Value::Object(out)
}

fn facet(rows: &[Row], col: &str) -> Value {
    json!(rows.iter().map(|r| json!({"value": r.get(col).cloned().unwrap_or(Value::Null), "n": r.i("n")})).collect::<Vec<_>>())
}

/// "Did you mean": the query's words that look like typos of words the
/// titles hold, corrected.
pub async fn did_you_mean(c: &Ctx<'_>, text: &str) -> Option<String> {
    if text.split_whitespace().count() == 0 {
        return None;
    }
    let t0 = std::time::Instant::now();
    let r = c.db.post("/v1/correct", &json!({"table": "hn_stories", "field": "title", "text": text})).await.ok()?;
    c.trace.push("did you mean (/v1/correct)", &format!("POST /v1/correct {{table: hn_stories, field: title, text: {text:?}}}"), &Value::Null, 1, t0.elapsed().as_secs_f64() * 1000.0, None);
    if r["changed"].as_bool() == Some(true) {
        r["text"].as_str().map(str::to_string)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_titles() {
        assert_eq!(short_title("Show HN: A thing"), "A thing");
        assert_eq!(short_title("Rust – a talk"), "Rust – a talk");
        assert_eq!(short_title("Show H–"), "Show H–");
    }
}
