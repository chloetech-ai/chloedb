//! Experiments on this site, by the Experiments app of ChloeDB's command
//! center (`chloedb-experiments`): what an arm of an experiment may change
//! of the search (its knobs), and the routes the app calls, under
//! `/v1/experiment`. They answer only when the site allows experiments
//! (`--experiments-token`), and only to a caller with that token, in
//! public too: an arm runs the site's own search on its own tables, with
//! the knobs it is given, and nothing an arm sets reaches anyone else's.
//!
//! The protocol is the one chloedb-hn speaks (`GET /v1/experiment`,
//! `POST /v1/experiment/search`, `GET /v1/experiment/item/<id>`,
//! `GET /v1/experiment/sample`).

use serde_json::{json, Value};

/// The paper table the search reads by default.
pub const MAIN_INDEX: &str = "ax_papers";

/// What an arm may change of the search. The default is the site as it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Knobs {
    /// Read the year, people, categories, kinds and sorts from the words.
    pub understand: bool,
    /// Candidates by meaning, and meaning in the ranking.
    pub meaning: bool,
    /// The title's words with a typo match (fuzzy).
    pub typos: bool,
    /// The field's synonyms (rag, llm, gnn) widen the words.
    pub synonyms: bool,
    /// Candidates by the authors' names, and their names in the ranking.
    pub authors: bool,
    /// The exponent of citations (cites + 1) in the ranking.
    pub cites: f64,
    /// The share of a five-year decay of age.
    pub recency: f64,
    pub meaning_weight: f64,
    pub phrase: f64,
    /// The weight of the abstract's words.
    pub abstract_weight: f64,
    /// The weight of the authors' names.
    pub author_weight: f64,
    /// The whole ranking expression, instead of the one the knobs make.
    pub rank: Option<String>,
    /// Blending: at most this many papers of one primary category (0: any).
    pub max_per_category: usize,
    /// Blending: at most this many papers of one first author (0: any).
    pub max_per_author: usize,
    /// Blending: places of the page kept for the best of the last year.
    pub fresh: usize,
    /// The table of papers the search reads.
    pub index: String,
}

impl Default for Knobs {
    fn default() -> Knobs {
        Knobs {
            understand: true,
            meaning: true,
            typos: true,
            synonyms: true,
            authors: true,
            cites: 0.18,
            recency: 0.15,
            meaning_weight: 3.0,
            phrase: 0.8,
            abstract_weight: 0.35,
            author_weight: 0.6,
            rank: None,
            max_per_category: 0,
            max_per_author: 0,
            fresh: 0,
            index: MAIN_INDEX.into(),
        }
    }
}

/// (key, group, title, about, min, max, step)
const NUMBERS: &[(&str, &str, &str, &str, f64, f64, f64)] = &[
    ("cites", "Ranking", "How much citations count", "the exponent of cites + 1 that multiplies how well the words fit (0: citations do not count)", 0.0, 0.6, 0.02),
    ("recency", "Ranking", "How much being recent counts", "the share of the score a five-year decay of age decides", 0.0, 1.0, 0.05),
    ("meaning_weight", "Ranking", "Meaning in the ranking", "the weight of the similarity of meaning beside the words' BM25", 0.0, 10.0, 0.5),
    ("phrase", "Ranking", "The words together in the title", "the weight of the words as a phrase in the title", 0.0, 3.0, 0.1),
    ("abstract_weight", "Ranking", "The abstract's words", "the weight of BM25 of the abstract", 0.0, 2.0, 0.05),
    ("author_weight", "Ranking", "The authors' names", "the weight of BM25 of the authors' names", 0.0, 2.0, 0.1),
];

impl Knobs {
    /// The knobs `v` sets over the site as it is; refused when one is not
    /// a knob, is out of its range, or names an index the site has not.
    pub fn from_json(v: &Value, indexes: &[(String, String)]) -> Result<Knobs, String> {
        let mut k = Knobs::default();
        let Some(o) = v.as_object() else {
            return if v.is_null() { Ok(k) } else { Err("knobs are an object of knob to value".into()) };
        };
        for (key, val) in o {
            if val.is_null() {
                continue;
            }
            let b = || val.as_bool().ok_or_else(|| format!("{key} is true or false"));
            let cap = |most: u64, what: &str| val.as_u64().filter(|n| *n <= most).map(|n| n as usize).ok_or_else(|| format!("{key} is 0 to {most} {what}"));
            match key.as_str() {
                "understand" => k.understand = b()?,
                "meaning" => k.meaning = b()?,
                "typos" => k.typos = b()?,
                "synonyms" => k.synonyms = b()?,
                "authors" => k.authors = b()?,
                "rank" => {
                    let s = val.as_str().ok_or("rank is an ORDER BY expression")?.trim();
                    if !s.is_empty() {
                        k.rank = Some(rank_expression(s)?);
                    }
                }
                "max_per_category" => k.max_per_category = cap(10, "papers (0: any)")?,
                "max_per_author" => k.max_per_author = cap(10, "papers (0: any)")?,
                "fresh" => k.fresh = cap(5, "places")?,
                "index" => {
                    let s = val.as_str().ok_or("index is the name of an index")?;
                    k.index = if s == "main" || s == MAIN_INDEX {
                        MAIN_INDEX.into()
                    } else {
                        indexes.iter().find(|(n, t)| n == s || t == s).map(|(_, t)| t.clone()).ok_or_else(|| format!("no index {s:?}: the site has {}", std::iter::once("main".to_string()).chain(indexes.iter().map(|(n, _)| n.clone())).collect::<Vec<_>>().join(", ")))?
                    };
                }
                other => {
                    let Some(&(_, _, _, _, min, max, _)) = NUMBERS.iter().find(|n| n.0 == other) else {
                        return Err(format!("{other:?} is not a knob of this site: GET /v1/experiment lists them"));
                    };
                    let x = val.as_f64().filter(|x| x.is_finite() && *x >= min && *x <= max).ok_or_else(|| format!("{other} is a number from {min} to {max}"))?;
                    *k.number(other) = x;
                }
            }
        }
        Ok(k)
    }

    fn number(&mut self, key: &str) -> &mut f64 {
        match key {
            "cites" => &mut self.cites,
            "recency" => &mut self.recency,
            "meaning_weight" => &mut self.meaning_weight,
            "phrase" => &mut self.phrase,
            "abstract_weight" => &mut self.abstract_weight,
            _ => &mut self.author_weight,
        }
    }

    /// The options of a BM25 or MATCH of the title's words.
    pub fn title_opts(&self) -> String {
        format!("{}{}", if self.typos { ", fuzzy => 'auto'" } else { "" }, if self.synonyms { ", synonyms => 1" } else { "" })
    }

    /// The options of the abstract's words.
    pub fn abstract_opts(&self) -> &'static str {
        if self.synonyms {
            ", synonyms => 1"
        } else {
            ""
        }
    }

    /// The ranking of the candidates: the knobs' (which, at the defaults,
    /// is the site's), or the arm's own expression.
    pub fn ranking(&self, meaning: bool) -> String {
        if let Some(r) = &self.rank {
            return format!("({r})");
        }
        let sim = if meaning { format!(" + {} * coalesce(similarity(emb, :v), 0)", num(self.meaning_weight)) } else { String::new() };
        let au = if self.authors { format!(" + {} * coalesce(bm25(authors, :q), 0)", num(self.author_weight)) } else { String::new() };
        format!(
            "(coalesce(bm25(title, :q{}), 0) + {} * coalesce(bm25(title, :phrase), 0) + {} * coalesce(bm25(abstract, :q{}), 0) + 0.25 * coalesce(bm25(abstract, :phrase), 0){au}{sim}) * pow(cites + 1, {}) * ({} + {} * decay(created, now(), 157680000))",
            self.title_opts(),
            num(self.phrase),
            num(self.abstract_weight),
            self.abstract_opts(),
            num(self.cites),
            num(1.0 - self.recency),
            num(self.recency)
        )
    }

    /// Whether the page is blended after ranking.
    pub fn blends(&self) -> bool {
        self.max_per_category > 0 || self.max_per_author > 0 || self.fresh > 0
    }

    /// Blending after ranking: at most so many papers of one category and
    /// of one first author, and `fresh` places for the best of the last
    /// year; the order within each kept as ranked.
    pub fn blend<T>(&self, rows: Vec<T>, category: impl Fn(&T) -> String, author: impl Fn(&T) -> String, created: impl Fn(&T) -> i64, now: i64, page: usize) -> Vec<T> {
        let rows = cap(rows, self.max_per_category, &category);
        let rows = cap(rows, self.max_per_author, &author);
        keep_fresh(rows, self.fresh, &created, now, page)
    }
}

/// At most `most` rows of one key in order; the others after them.
fn cap<T>(rows: Vec<T>, most: usize, key: &impl Fn(&T) -> String) -> Vec<T> {
    if most == 0 {
        return rows;
    }
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let (mut keep, mut later) = (Vec::new(), Vec::new());
    for r in rows {
        let k = key(&r);
        let n = seen.entry(k.clone()).or_insert(0);
        if k.is_empty() || *n < most {
            *n += 1;
            keep.push(r);
        } else {
            later.push(r);
        }
    }
    keep.extend(later);
    keep
}

/// The best `n` rows of the last year brought into the page's last places
/// when the ranking left them below it.
fn keep_fresh<T>(mut rows: Vec<T>, n: usize, created: &impl Fn(&T) -> i64, now: i64, page: usize) -> Vec<T> {
    if n == 0 {
        return rows;
    }
    let year = now - 365 * 86_400;
    let fresh_at: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| created(r) >= year).map(|(i, _)| i).take(n).collect();
    let shown = page.min(rows.len());
    let already = fresh_at.iter().filter(|&&i| i < shown).count();
    let missing: Vec<usize> = fresh_at.into_iter().filter(|&i| i >= shown).take(n.saturating_sub(already)).collect();
    if missing.is_empty() {
        return rows;
    }
    let mut taken = Vec::new();
    for &i in missing.iter().rev() {
        taken.push(rows.remove(i));
    }
    taken.reverse();
    let at = shown.saturating_sub(taken.len()).min(rows.len());
    for (j, r) in taken.into_iter().enumerate() {
        rows.insert(at + j, r);
    }
    rows
}

/// A number as SQL writes it: no trailing zeros.
pub fn num(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// An arm's ranking expression, as given: an expression of ChloeDB's SQL,
/// never a statement or a part of one.
pub fn rank_expression(s: &str) -> Result<String, String> {
    if s.len() > 2000 {
        return Err("a ranking expression is 2,000 characters at most".into());
    }
    if s.contains(';') || s.contains("--") || s.contains("/*") {
        return Err("a ranking expression has no ;, -- or /*".into());
    }
    let upper = s.to_ascii_uppercase();
    for w in upper.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if matches!(w, "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "DROP" | "CREATE" | "ALTER" | "INTO" | "FROM" | "WITH" | "UNION" | "LIMIT" | "ORDER") {
            return Err(format!("a ranking expression is one expression of the row: {w} is not part of one"));
        }
    }
    let mut depth = 0i32;
    for c in s.chars() {
        depth += match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        if depth < 0 {
            return Err("a ranking expression's parentheses do not match".into());
        }
    }
    if depth != 0 {
        return Err("a ranking expression's parentheses do not match".into());
    }
    Ok(s.to_string())
}

/// The site and its knobs, as `GET /v1/experiment` says them.
pub fn describe(indexes: &[(String, String)]) -> Value {
    let mut d = Knobs::default();
    let mut knobs = vec![
        json!({"key": "understand", "group": "Query understanding", "title": "Read filters from the words", "about": "“by hinton”, “since 2020”, “cs.CL”, “surveys”, “newest” and questions in a sentence become filters and sorts; off, every word is searched as typed", "kind": "bool", "default": d.understand}),
        json!({"key": "typos", "group": "Retrieval", "title": "Typos", "about": "the title's words one or two letters away match too (fuzzy)", "kind": "bool", "default": d.typos}),
        json!({"key": "synonyms", "group": "Retrieval", "title": "The field's synonyms", "about": "rag, llm, gnn and the field's other synonyms widen the words", "kind": "bool", "default": d.synonyms}),
        json!({"key": "meaning", "group": "Retrieval", "title": "Search by meaning", "about": "papers near the words in meaning are candidates, and meaning counts in the ranking (off: words alone)", "kind": "bool", "default": d.meaning}),
        json!({"key": "authors", "group": "Retrieval", "title": "Authors' names", "about": "papers whose authors the words name are candidates, and the names count in the ranking", "kind": "bool", "default": d.authors}),
    ];
    for &(key, group, title, about, min, max, step) in NUMBERS {
        let def = *d.number(key);
        knobs.push(json!({"key": key, "group": group, "title": title, "about": about, "kind": "number", "default": def, "min": min, "max": max, "step": step}));
    }
    knobs.push(json!({"key": "rank", "group": "Ranking", "title": "Ranking expression", "about": "replaces the ranking the knobs above make: an ORDER BY expression of ChloeDB's SQL over ax_papers (:q the words, :phrase them quoted, :v their meaning)", "kind": "sql", "default": d.ranking(true)}));
    knobs.push(json!({"key": "max_per_category", "group": "Blending", "title": "At most this many of one category", "about": "after ranking, a primary category's papers past this many in the page move below the others (0: any number)", "kind": "int", "default": 0, "min": 0, "max": 10}));
    knobs.push(json!({"key": "max_per_author", "group": "Blending", "title": "At most this many of one first author", "about": "after ranking, a first author's papers past this many in the page move below the others (0: any number)", "kind": "int", "default": 0, "min": 0, "max": 10}));
    knobs.push(json!({"key": "fresh", "group": "Blending", "title": "Places for the last year", "about": "after ranking, this many places of the page go to the best papers of the last year, if the ranking left them below", "kind": "int", "default": 0, "min": 0, "max": 5}));
    let mut index_choices = vec![json!({"value": "main", "title": MAIN_INDEX, "about": "the papers as the site indexes them"})];
    for (n, t) in indexes {
        index_choices.push(json!({"value": n, "title": t, "about": format!("{t}, an index of the papers built another way")}));
    }
    knobs.push(json!({"key": "index", "group": "Index", "title": "Index", "about": "the table of papers the search reads: an index built with other analyzers, fields or embeddings, beside the main one", "kind": "choice", "default": "main", "choices": index_choices}));
    json!({
        "protocol": 1,
        "site": "arxiv",
        "title": "arXiv",
        "about": "arXiv: every paper with its title, abstract, authors, categories and citations, searched by words and meaning. People come to find a paper they half remember, the papers on a method or problem, what a person or lab wrote, surveys to start from, or the newest work on a subject.",
        "rows": "papers",
        "knobs": knobs,
        "presets": [
            {"name": "As it is", "knobs": {}},
            {"name": "Words only", "knobs": {"meaning": false}},
            {"name": "No typos or synonyms", "knobs": {"typos": false, "synonyms": false}},
            {"name": "Citations count less", "knobs": {"cites": 0.06}},
            {"name": "Newer first", "knobs": {"recency": 0.5}},
            {"name": "Words as typed", "knobs": {"understand": false}},
            {"name": "Two per category", "knobs": {"max_per_category": 2}},
        ],
        "indexes": std::iter::once(json!({"name": "main", "table": MAIN_INDEX})).chain(indexes.iter().map(|(n, t)| json!({"name": n, "table": t}))).collect::<Vec<_>>(),
        "item": "/v1/experiment/item/{id}",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_site_as_it_is() {
        let k = Knobs::from_json(&json!({}), &[]).unwrap();
        assert_eq!(k, Knobs::default());
        assert_eq!(
            k.ranking(true),
            "(coalesce(bm25(title, :q, fuzzy => 'auto', synonyms => 1), 0) + 0.8 * coalesce(bm25(title, :phrase), 0) + 0.35 * coalesce(bm25(abstract, :q, synonyms => 1), 0) + 0.25 * coalesce(bm25(abstract, :phrase), 0) + 0.6 * coalesce(bm25(authors, :q), 0) + 3 * coalesce(similarity(emb, :v), 0)) * pow(cites + 1, 0.18) * (0.85 + 0.15 * decay(created, now(), 157680000))"
        );
    }

    #[test]
    fn knobs_are_checked() {
        let ix = vec![("v2".to_string(), "ax_papers_v2".to_string())];
        let k = Knobs::from_json(&json!({"meaning": false, "cites": 0.1, "typos": false, "authors": false, "index": "v2", "max_per_category": 2}), &ix).unwrap();
        assert!(!k.meaning && !k.typos && k.synonyms && !k.authors && k.index == "ax_papers_v2" && k.max_per_category == 2);
        let r = k.ranking(false);
        assert!(r.contains("pow(cites + 1, 0.1)") && !r.contains("fuzzy") && !r.contains("authors") && !r.contains("similarity"), "{r}");
        for bad in [json!({"cites": 3}), json!({"nope": 1}), json!({"meaning": 1}), json!({"index": "v3"}), json!({"rank": "cites; DROP TABLE x"}), json!({"rank": "(SELECT 1)"}), json!({"fresh": 9})] {
            assert!(Knobs::from_json(&bad, &ix).is_err(), "{bad}");
        }
    }

    #[test]
    fn blending_caps_categories_and_authors() {
        let now = 400 * 86_400;
        // (category, first author, created)
        let rows: Vec<(&str, &str, i64)> = vec![("cl", "x", 0), ("cl", "y", 0), ("cl", "x", 0), ("lg", "x", 0), ("cv", "z", now)];
        let ids = |v: &[(&str, &str, i64)]| v.iter().map(|r| format!("{}{}", r.0, r.1)).collect::<Vec<_>>().join(" ");
        let k = Knobs { max_per_category: 2, ..Knobs::default() };
        assert_eq!(ids(&k.blend(rows.clone(), |r| r.0.into(), |r| r.1.into(), |r| r.2, now, 3)), "clx cly lgx cvz clx");
        let k = Knobs { max_per_author: 1, ..Knobs::default() };
        assert_eq!(ids(&k.blend(rows.clone(), |r| r.0.into(), |r| r.1.into(), |r| r.2, now, 3)), "clx cly cvz clx lgx");
        let k = Knobs { fresh: 1, ..Knobs::default() };
        assert_eq!(ids(&k.blend(rows, |r| r.0.into(), |r| r.1.into(), |r| r.2, now, 3)), "clx cly cvz clx lgx");
    }
}
