//! Search: a search box's words, understood (`understand`), as SQL over the
//! papers. Every statement it runs is kept in the request's trace, so the
//! page can show the SQL behind each answer.
//!
//! Ranking a paper, "best" (the default):
//!
//! 1. Candidates by words: BM25 over the title (typos corrected, the
//!    field's synonyms widened: `rag`, `llm`, `gnn`), over the abstract and
//!    over the authors, each times a prior of citations (`pop`,
//!    ln(1+cites)), a rank-safe boost: the exact top rows by the product.
//! 2. Candidates by meaning: the nearest titles and abstracts to the
//!    query's vector (potion-retrieval-32M, embedded where the query runs).
//! 3. Both reranked together: the title's BM25 counts most, the words as a
//!    phrase in the title more, then the abstract, the meaning, the
//!    citations (gently: a 30,000-citation paper is not 30 times as
//!    relevant as one with a thousand) and a light preference for the
//!    recent.

use serde_json::{json, Map, Value};

use crate::strata::{Row, RowExt, Strata, Trace};
use crate::understand::Parsed;

pub struct Ctx<'a> {
    /// The search server: papers, people, topics.
    pub db: &'a Strata,
    /// The graph server: reading events and the recommender, citations,
    /// coauthors, and what readers keep (feeds, issues, notes). The same
    /// server as `db` when one holds everything.
    pub g: &'a Strata,
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
    /// A query of the graph server.
    pub fn grows(&self, what: &str, sql: &str, params: Value) -> impl std::future::Future<Output = Result<Vec<Row>, crate::strata::Error>> + 'a {
        let what = if self.g.url == self.db.url { what.to_string() } else { format!("{what} (graph server)") };
        let (db, trace, sql) = (self.g, self.trace, sql.to_string());
        async move { db.traced(trace, &what, &sql, params).await }
    }
    pub fn gsoft(&self, what: &str, sql: &str, params: Value) -> impl std::future::Future<Output = Vec<Row>> + 'a {
        let f = self.grows(what, sql, params);
        async move { f.await.unwrap_or_default() }
    }
}

/// The columns a list of papers shows.
pub const COLS: &str = "id, title, authors, people, nauthors, pcat, cats, area, created, updated, year, versions, venue, venue_year, journal, code, hf, techniques, domains, kinds, cites, cited_in, refs, topic, pages, doi";
/// The same with the abstract.
pub const COLS_ABS: &str = "id, title, abstract, authors, people, nauthors, pcat, cats, area, created, updated, year, versions, venue, venue_year, journal, code, hf, techniques, domains, kinds, cites, cited_in, refs, topic, pages, doi";

pub fn iso(t: i64) -> String {
    crate::paper::iso(t)
}

/// A `%text%` pattern for `ILIKE` (a `%` in the text matches any one
/// character: the substring index takes no `ESCAPE`).
pub fn like(x: &str) -> String {
    format!("%{}%", x.replace('%', "_"))
}

/// The filters a query's understanding gives, as SQL with parameters.
pub fn filters(p: &Parsed, params: &mut Map<String, Value>) -> Vec<String> {
    let mut w = Vec::new();
    for (i, a) in p.authors.iter().enumerate() {
        params.insert(format!("au{i}"), json!(a));
        w.push(format!("people = :au{i}"));
    }
    if !p.cats.is_empty() {
        let mut any = Vec::new();
        for (i, c) in p.cats.iter().enumerate() {
            params.insert(format!("cat{i}"), json!(c));
            any.push(format!("cats = :cat{i}"));
        }
        w.push(format!("({})", any.join(" OR ")));
    }
    if let Some(a) = &p.area {
        params.insert("area".into(), json!(a));
        w.push("areas = :area".into());
    }
    for (i, t) in p.techniques.iter().enumerate() {
        params.insert(format!("tech{i}"), json!(t));
        w.push(format!("techniques = :tech{i}"));
    }
    for (i, d) in p.domains.iter().enumerate() {
        params.insert(format!("dom{i}"), json!(d));
        w.push(format!("domains = :dom{i}"));
    }
    for (i, k) in p.kinds.iter().enumerate() {
        params.insert(format!("kind{i}"), json!(k));
        w.push(format!("kinds = :kind{i}"));
    }
    if let Some(v) = &p.venue {
        params.insert("venue".into(), json!(v));
        w.push("venue = :venue".into());
    }
    if let Some(a) = p.after {
        w.push(format!("created >= TIMESTAMP '{}'", iso(a)));
    }
    if let Some(b) = p.before {
        w.push(format!("created < TIMESTAMP '{}'", iso(b)));
    }
    if let Some(n) = p.cites_min {
        w.push(format!("cites >= {n}"));
    }
    if p.has_code {
        w.push("code != ''".into());
    }
    if let Some(t) = p.topic {
        w.push(format!("topic = {t}"));
    }
    // Text in backticks, exactly: the title's substring index answers
    // `ILIKE` without reading the rows.
    for (i, x) in p.exact.iter().enumerate() {
        params.insert(format!("ex{i}"), json!(like(x)));
        w.push(format!("title ILIKE :ex{i}"));
    }
    for (i, x) in p.exclude.iter().enumerate() {
        let x: String = x.chars().filter(|c| c.is_alphanumeric() || matches!(c, '+' | '#' | '.' | '-')).collect();
        if x.is_empty() {
            continue;
        }
        params.insert(format!("x{i}"), json!(x));
        w.push(format!("NOT MATCH(title, :x{i})"));
    }
    w
}

pub fn where_of(conds: &[String], extra: &str) -> String {
    let all: Vec<&str> = conds.iter().map(String::as_str).chain([extra]).filter(|s| !s.is_empty()).collect();
    if all.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", all.join(" AND "))
    }
}

/// How many of a query's words a paper must hold: all of up to three, three
/// of four, 60% of more. Longer queries are sentences, where meaning does
/// the work.
pub fn min_words(text: &str) -> String {
    match text.split_whitespace().count() {
        0..=3 => format!("{}", text.split_whitespace().count().max(1)),
        4 => "3".into(),
        _ => "60%".into(),
    }
}

/// A paper the words are in: its title or its abstract, each holding `:min`
/// of the words `:m` (synonyms counted).
pub const MATCHED: &str = "(MATCH(title, :m, synonyms => 1, min => :min) OR MATCH(abstract, :m, synonyms => 1, min => :min))";
pub const MATCHED_FUZZY: &str = "(MATCH(title, :m, fuzzy => 'auto', synonyms => 1, min => :min) OR MATCH(abstract, :m, fuzzy => 'auto', synonyms => 1, min => :min))";

#[derive(Clone, Debug, Default)]
pub struct Request {
    /// `best`, `date`, `cites`, `old`.
    pub sort: String,
    /// `hybrid`, `words`, `meaning`, `exact`.
    pub mode: String,
}

/// The query's parameters for its words: `:q` (the words), `:m` (for
/// MATCH, phrases quoted), `:min`, `:v` (its vector, embedded by the
/// server), `:phrase`.
pub fn word_params(p: &Parsed, params: &mut Map<String, Value>) -> bool {
    let words = p.words();
    if words.is_empty() {
        return false;
    }
    params.insert("q".into(), json!(words));
    params.insert("m".into(), json!(p.match_text()));
    params.insert("min".into(), json!(min_words(&words)));
    params.insert("v".into(), json!({"embed": words, "field": "emb", "table": "ax_papers"}));
    params.insert("phrase".into(), json!(format!("\"{}\"", words.replace('"', ""))));
    true
}

/// The papers for a query, and how many match.
pub async fn papers(c: &Ctx<'_>, p: &Parsed, r: &Request, limit: usize, offset: usize) -> Result<(Vec<Row>, i64), crate::strata::Error> {
    let mut params = Map::new();
    let conds = filters(p, &mut params);
    let has_text = word_params(p, &mut params);
    let mode = if r.mode.is_empty() { "hybrid" } else { r.mode.as_str() };
    let order_simple = match r.sort.as_str() {
        "date" => Some("created DESC"),
        "old" => Some("created ASC"),
        "cites" => Some("cites DESC"),
        _ => None,
    };
    let pv = |m: &Map<String, Value>| Value::Object(m.clone());
    if !has_text {
        let ord = order_simple.unwrap_or(if conds.is_empty() { "created DESC" } else { "cites DESC" });
        let sql = format!("SELECT {COLS_ABS} FROM ax_papers {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_of(&conds, ""));
        let (rows, total) = tokio::join!(c.rows("papers", &sql, pv(&params)), count(c, &where_of(&conds, ""), &params));
        return Ok((rows?, total));
    }
    if mode == "exact" {
        params.insert("xall".into(), json!(like(&p.words())));
        let cond = "title ILIKE :xall";
        let ord = order_simple.unwrap_or("cites DESC");
        let sql = format!("SELECT {COLS_ABS} FROM ax_papers {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_of(&conds, cond));
        let (rows, total) = tokio::join!(c.rows("papers, the title exactly (substring index)", &sql, pv(&params)), count(c, &where_of(&conds, cond), &params));
        return Ok((rows?, total));
    }
    if mode == "meaning" {
        let sql = format!("SELECT {COLS_ABS}, score() AS score FROM ax_papers {} ORDER BY similarity(emb, :v) DESC LIMIT {limit} OFFSET {offset}", where_of(&conds, ""));
        let rows = c.rows("papers by meaning", &sql, pv(&params)).await?;
        return Ok((rows, 0));
    }
    if let Some(ord) = order_simple {
        // Newest (or most cited) of those the words are in.
        let sql = format!("SELECT {COLS_ABS} FROM ax_papers {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_of(&conds, MATCHED));
        let (rows, total) = tokio::join!(c.rows("papers with the words", &sql, pv(&params)), count(c, &where_of(&conds, MATCHED), &params));
        let rows = rows?;
        if !rows.is_empty() {
            return Ok((rows, total));
        }
        let sql = format!("SELECT {COLS_ABS} FROM ax_papers {} ORDER BY {ord}, id DESC LIMIT {limit} OFFSET {offset}", where_of(&conds, MATCHED_FUZZY));
        let (rows, total) = tokio::join!(c.rows("papers with the words, typos corrected", &sql, pv(&params)), count(c, &where_of(&conds, MATCHED_FUZZY), &params));
        return Ok((rows?, total));
    }
    // Phase 1: candidates by words (title, abstract, authors) and by meaning.
    let cand = (offset + limit).max(100) * 2;
    let wh = where_of(&conds, "");
    let title_sql = format!("SELECT id FROM ax_papers {wh} ORDER BY bm25(title, :q, fuzzy => 'auto', synonyms => 1, min => :min) * (1 + 0.25 * pop) DESC LIMIT {cand}");
    let abs_sql = format!("SELECT id FROM ax_papers {wh} ORDER BY bm25(abstract, :q, synonyms => 1, min => :min) * (1 + 0.25 * pop) DESC LIMIT {}", cand / 2);
    let au_sql = format!("SELECT id FROM ax_papers {wh} ORDER BY bm25(authors, :q) * (1 + 0.3 * pop) DESC LIMIT 30");
    let vec_sql = format!("SELECT id FROM ax_papers {wh} ORDER BY similarity(emb, :v) * (1 + 0.04 * pop) DESC LIMIT 100");
    let pr = pv(&params);
    let (t, a, au, v) = tokio::join!(
        c.rows("candidates by the title's words", &title_sql, pr.clone()),
        c.soft("candidates by the abstract's words", &abs_sql, pr.clone()),
        c.soft("candidates by authors' names", &au_sql, pr.clone()),
        async { if mode == "hybrid" { c.soft("candidates by meaning", &vec_sql, pr.clone()).await } else { Vec::new() } },
    );
    let mut ids: Vec<String> = t?.iter().chain(a.iter()).chain(au.iter()).chain(v.iter()).map(|r| r.s("id")).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok((Vec::new(), 0));
    }
    params.insert("ids".into(), json!(ids));
    let sim = if mode == "hybrid" { " + 3.0 * coalesce(similarity(emb, :v), 0)" } else { "" };
    // Phase 2: the candidates ranked together.
    let rank = format!(
        "(coalesce(bm25(title, :q, fuzzy => 'auto', synonyms => 1), 0) + 0.8 * coalesce(bm25(title, :phrase), 0) + 0.35 * coalesce(bm25(abstract, :q, synonyms => 1), 0) + 0.25 * coalesce(bm25(abstract, :phrase), 0) + 0.6 * coalesce(bm25(authors, :q), 0){sim}) * pow(cites + 1, 0.18) * (0.85 + 0.15 * decay(created, now(), 157680000))"
    );
    let sql = format!("SELECT {COLS_ABS}, score() AS score FROM ax_papers WHERE id IN (:ids) ORDER BY {rank} DESC LIMIT {limit} OFFSET {offset}");
    let (rows, total) = tokio::join!(c.rows("papers, ranked", &sql, pv(&params)), count(c, &where_of(&conds, MATCHED), &params));
    let rows = rows?;
    let mut total = total;
    if total == 0 && !rows.is_empty() {
        total = count(c, &where_of(&conds, MATCHED_FUZZY), &params).await;
    }
    Ok((rows, total))
}

pub fn count<'a>(c: &Ctx<'a>, wh: &str, params: &Map<String, Value>) -> impl std::future::Future<Output = i64> + 'a {
    let f = c.soft("how many match", &format!("SELECT COUNT(*) AS n FROM ax_papers {wh}"), Value::Object(params.clone()));
    async move { f.await.first().map(|r| r.i("n")).unwrap_or(0) }
}

/// The condition of the papers a query is about: its filters and its words
/// (title or abstract).
pub fn about(p: &Parsed, params: &mut Map<String, Value>) -> String {
    let conds = filters(p, params);
    let has = word_params(p, params);
    where_of(&conds, if has { MATCHED } else { "" })
}

/// Counts for the papers a query matches: by year, category, area,
/// technique, domain, kind, venue and author, and those with code.
pub async fn facets(c: &Ctx<'_>, p: &Parsed) -> Value {
    let mut params = Map::new();
    let wh = about(p, &mut params);
    let pr = Value::Object(params.clone());
    let code_wh = if wh.is_empty() { "WHERE code != ''".to_string() } else { format!("{wh} AND code != ''") };
    let (years, cats, techs, doms, kinds, venues, people, code) = tokio::join!(
        c.soft("papers by year", &format!("SELECT year, COUNT(*) AS n FROM ax_papers {wh} GROUP BY year ORDER BY year"), pr.clone()),
        c.soft("papers by category", &format!("SELECT cats, COUNT(*) AS n FROM ax_papers {wh} GROUP BY cats ORDER BY n DESC LIMIT 14"), pr.clone()),
        c.soft("papers by technique", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers {wh} GROUP BY techniques ORDER BY n DESC LIMIT 14"), pr.clone()),
        c.soft("papers by domain", &format!("SELECT domains, COUNT(*) AS n FROM ax_papers {wh} GROUP BY domains ORDER BY n DESC LIMIT 14"), pr.clone()),
        c.soft("papers by kind", &format!("SELECT kinds, COUNT(*) AS n FROM ax_papers {wh} GROUP BY kinds ORDER BY n DESC"), pr.clone()),
        c.soft("papers by venue", &format!("SELECT venue, COUNT(*) AS n FROM ax_papers {wh} GROUP BY venue ORDER BY n DESC LIMIT 13"), pr.clone()),
        c.soft("papers by author", &format!("SELECT people, COUNT(*) AS n FROM ax_papers {wh} GROUP BY people ORDER BY n DESC LIMIT 12"), pr.clone()),
        c.soft("papers with code", &format!("SELECT COUNT(*) AS n FROM ax_papers {code_wh}"), pr.clone()),
    );
    let dict = crate::concepts::dict();
    let named = |rows: &[Row], col: &str, name: &dyn Fn(&str) -> String| -> Value {
        json!(rows.iter().filter(|r| !r.s(col).is_empty()).map(|r| {
            let v = r.s(col);
            json!({"value": v, "name": name(&v), "n": r.i("n")})
        }).collect::<Vec<_>>())
    };
    json!({
        "year": json!(years.iter().filter(|r| r.i("year") > 0).map(|r| json!({"value": r.i("year"), "n": r.i("n")})).collect::<Vec<_>>()),
        "category": named(&cats, "cats", &|v| crate::concepts::category_name(v)),
        "technique": named(&techs, "techniques", &|v| dict.by_slug(v).map(|c| c.name.clone()).unwrap_or_else(|| v.to_string())),
        "domain": named(&doms, "domains", &|v| dict.by_slug(v).map(|c| c.name.clone()).unwrap_or_else(|| v.to_string())),
        "kind": named(&kinds, "kinds", &|v| crate::understand::kind_name(v).to_string()),
        "venue": named(&venues, "venue", &|v| v.to_string()),
        "author": json!(people.iter().filter(|r| !r.s("people").is_empty()).map(|r| json!({"value": r.s("people"), "n": r.i("n")})).collect::<Vec<_>>()),
        "code": code.first().map(|r| r.i("n")).unwrap_or(0),
    })
}

/// The names of people's keys, from the authors' table.
pub async fn names(c: &Ctx<'_>, keys: &[String]) -> std::collections::HashMap<String, Row> {
    if keys.is_empty() {
        return Default::default();
    }
    let rows = c.soft("their names", "SELECT id, name, papers, cites, h FROM ax_authors WHERE id IN (:k)", json!({"k": keys})).await;
    rows.into_iter().map(|r| (r.s("id"), r)).collect()
}

/// A person the words may name: the author of that key, else the one of
/// that name with the most papers. Returns their key and name.
pub async fn person(c: &Ctx<'_>, words: &str) -> Option<(String, String)> {
    let first = words.split_whitespace().next().unwrap_or("");
    let key = crate::concepts::person_key(if words.split_whitespace().count() > 1 { &words[..words.rfind(' ').unwrap_or(0)] } else { "" }, words.split_whitespace().last().unwrap_or(""));
    let key_full = crate::concepts::person_key("", words);
    let exact = c.soft("is that a person?", "SELECT id, name, papers FROM ax_authors WHERE id IN (:k) ORDER BY papers DESC LIMIT 1", json!({"k": [key, key_full]})).await;
    if let Some(r) = exact.first() {
        return Some((r.s("id"), r.s("name")));
    }
    // A last name alone, or initials: the most prolific of that name.
    if words.trim().is_empty() || first.len() < 2 {
        return None;
    }
    let rows = c.soft("people of that name", "SELECT id, name, papers FROM ax_authors WHERE MATCH(name, :n, min => '100%') ORDER BY papers DESC LIMIT 3", json!({"n": words})).await;
    let top = rows.first()?;
    // Only one clear person: a common last name ("wang") is not a filter.
    let second = rows.get(1).map(|r| r.i("papers")).unwrap_or(0);
    if words.split_whitespace().count() >= 2 || top.i("papers") >= 4 * second.max(1) {
        Some((top.s("id"), top.s("name")))
    } else {
        None
    }
}

const STOP: &[&str] = &["a", "an", "the", "of", "on", "in", "for", "to", "and", "or", "with", "by", "from", "via", "is", "are", "as", "at", "its", "into", "using", "how", "what", "which", "who", "we", "our", "this", "that", "be", "not"];

/// A word's stem for marking: lower case without the common endings.
fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    for end in ["ions", "ion", "ings", "ing", "ies", "es", "ed", "s"] {
        if w.len() > end.len() + 3 && w.ends_with(end) {
            return w[..w.len() - end.len()].to_string();
        }
    }
    w
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The texts with the query's words marked (their stems, so "networks"
/// marks "network"), HTML-escaped between `<mark>` tags; with `size`, the
/// window of about that many characters that holds the most of them.
pub fn highlight(query: &str, texts: Vec<String>, size: usize) -> Option<Vec<String>> {
    let stems: Vec<String> = query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|w| w.len() >= 2 && !STOP.contains(&w.to_lowercase().as_str()))
        .map(stem)
        .collect();
    if stems.is_empty() || texts.is_empty() {
        return None;
    }
    let hits = |w: &str| {
        let l = w.to_lowercase();
        stems.iter().any(|s| l == *s || (s.chars().count() >= 3 && l.starts_with(s.as_str()) && l.chars().count() <= s.chars().count() + 4))
    };
    Some(
        texts
            .into_iter()
            .map(|t| {
                // Words with their byte spans.
                let mut spans: Vec<(usize, usize, bool)> = Vec::new();
                let mut st = None;
                for (i, ch) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
                    if ch.is_alphanumeric() {
                        if st.is_none() {
                            st = Some(i);
                        }
                    } else if let Some(s0) = st.take() {
                        spans.push((s0, i, hits(&t[s0..i])));
                    }
                }
                let (mut from, mut to) = (0, t.len());
                if size > 0 && t.len() > size + 40 {
                    // The window of `size` bytes, from a word's start, with the most marks.
                    let mut best = (0usize, 0usize);
                    for (k, sp) in spans.iter().enumerate() {
                        let n = spans[k..].iter().take_while(|x| x.1 <= sp.0 + size).filter(|x| x.2).count();
                        if n > best.1 {
                            best = (k, n);
                        }
                    }
                    let k = best.0.saturating_sub(if best.1 > 0 { 3 } else { 0 });
                    from = spans.get(k).map(|x| x.0).unwrap_or(0);
                    if k == 0 {
                        from = 0;
                    }
                    to = spans.iter().rev().find(|x| x.1 <= from + size).map(|x| x.1).unwrap_or(t.len()).max(from);
                }
                let mut out = String::new();
                if from > 0 {
                    out.push('…');
                }
                let mut at = from;
                for &(a, b, hit) in spans.iter().filter(|x| x.0 >= from && x.1 <= to) {
                    if hit {
                        out.push_str(&esc(&t[at..a]));
                        out.push_str("<mark>");
                        out.push_str(&esc(&t[a..b]));
                        out.push_str("</mark>");
                        at = b;
                    }
                }
                out.push_str(&esc(&t[at..to]));
                if to < t.len() {
                    out.push('…');
                }
                out
            })
            .collect(),
    )
}

/// Marks the titles and abstracts for the query.
pub async fn mark(c: &Ctx<'_>, p: &Parsed, rows: &mut [Row]) {
    let q = p.match_text();
    if q.trim().is_empty() || rows.is_empty() {
        return;
    }
    let titles: Vec<String> = rows.iter().map(|r| r.s("title")).collect();
    let abs: Vec<String> = rows.iter().map(|r| r.s("abstract")).collect();
    let _ = c;
    let (t, a) = (highlight(&q, titles, 0), highlight(&q, abs, 320));
    for (i, r) in rows.iter_mut().enumerate() {
        if let Some(x) = t.as_ref().and_then(|v| v.get(i)) {
            r.insert("title_hl".into(), json!(x));
        }
        if let Some(x) = a.as_ref().and_then(|v| v.get(i)).filter(|x| !x.is_empty()) {
            r.insert("abstract_hl".into(), json!(x));
        }
    }
}

/// "Did you mean": the words that look like typos of words titles hold,
/// corrected.
pub async fn did_you_mean(c: &Ctx<'_>, text: &str) -> Option<String> {
    if text.split_whitespace().count() == 0 {
        return None;
    }
    let t0 = std::time::Instant::now();
    let r = c.db.post("/v1/correct", &json!({"table": "ax_papers", "field": "title", "text": text})).await.ok()?;
    c.trace.push("did you mean (/v1/correct)", &format!("POST /v1/correct {{table: ax_papers, field: title, text: {text:?}}}"), &Value::Null, 1, t0.elapsed().as_secs_f64() * 1000.0, None);
    if r["changed"].as_bool() == Some(true) {
        r["text"].as_str().map(str::to_string)
    } else {
        None
    }
}

/// Short rows: drop the abstract to its first sentences for lists.
pub fn trim_abstract(rows: &mut [Row], chars: usize) {
    for r in rows.iter_mut() {
        let a = r.s("abstract");
        if a.chars().count() > chars {
            let cut: String = a.chars().take(chars).collect();
            let cut = match cut.rfind(". ") {
                Some(i) if i > chars / 2 => cut[..=i].to_string(),
                _ => format!("{}…", cut.trim_end()),
            };
            r.insert("abstract".into(), json!(cut));
        }
    }
}
