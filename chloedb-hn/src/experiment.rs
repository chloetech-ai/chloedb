//! Experiments on this site, by the Experiments app of ChloeDB's command
//! center (`chloedb-experiments`): what an arm of an experiment may change
//! of the search (its knobs), and the routes the app calls, under
//! `/v1/experiment`. They answer only when the site allows experiments
//! (`--experiments-token`), and only to a caller with that token, in
//! public too: an arm runs the site's own search on its own tables, with
//! the knobs it is given, and nothing an arm sets reaches anyone else's.
//!
//! The protocol, which any app of ChloeDB may speak:
//!
//! - `GET /v1/experiment`: the site (`site`, `title`, `about`, what its rows
//!   are), its knobs, each with its kind, default, range and group, the
//!   presets (arms worth trying), and the indexes an arm may read.
//! - `POST /v1/experiment/search` `{"q": ..., "knobs": {...}, "per": 10}`:
//!   the first page for those knobs, as `rows` of `id`, `title`, `snippet`,
//!   `url`, and what the search understood.
//! - `GET /v1/experiment/item/<id>`: what a reader sees on opening a row.
//! - `GET /v1/experiment/sample?n=60&seed=7`: rows of the site, popular
//!   and of every year, to write personas and their needs from.

use serde_json::{json, Value};

/// The story table the search reads by default.
pub const MAIN_INDEX: &str = "hn_stories";

/// What an arm may change of the search. The default is the site as it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Knobs {
    /// Read filters and sorts from the words ("by pg", "in 2015", "ask hn").
    pub understand: bool,
    /// Candidates by meaning, and meaning in the ranking.
    pub meaning: bool,
    /// Words with a typo match (fuzzy).
    pub typos: bool,
    pub synonyms: bool,
    /// Code as written (`std::move`) found by the substring index.
    pub exact: bool,
    /// Every word in the title first, then the rest.
    pub tiers: bool,
    /// The exponent of the votes (points + 1) in the ranking.
    pub votes: f64,
    /// The weight of being recent (a five-year decay).
    pub recency: f64,
    pub meaning_weight: f64,
    pub phrase: f64,
    pub body: f64,
    pub link: f64,
    /// The whole ranking expression of the candidates, instead of the one
    /// the knobs above make (`:q`, `:phrase` and, with meaning, `:v`).
    pub rank: Option<String>,
    /// Blending: at most this many stories of one site in the page (0: any).
    pub max_per_site: usize,
    /// Blending: places of the page kept for the best stories of the last
    /// year.
    pub fresh: usize,
    /// The table of stories the search reads (an index built another way).
    pub index: String,
}

impl Default for Knobs {
    fn default() -> Knobs {
        Knobs {
            understand: true,
            meaning: true,
            typos: true,
            synonyms: true,
            exact: true,
            tiers: true,
            votes: 0.35,
            recency: 0.2,
            meaning_weight: 2.5,
            phrase: 0.8,
            body: 0.2,
            link: 0.3,
            rank: None,
            max_per_site: 0,
            fresh: 0,
            index: MAIN_INDEX.into(),
        }
    }
}

/// (key, group, title, about, kind, min, max, step)
const NUMBERS: &[(&str, &str, &str, &str, f64, f64, f64)] = &[
    ("votes", "Ranking", "How much the votes count", "the exponent of points + 1 that multiplies how well the words fit (0: votes do not count)", 0.0, 1.0, 0.05),
    ("recency", "Ranking", "How much being recent counts", "the share of the score a five-year decay of age decides", 0.0, 1.0, 0.05),
    ("meaning_weight", "Ranking", "Meaning in the ranking", "the weight of the similarity of meaning beside the words' BM25", 0.0, 10.0, 0.5),
    ("phrase", "Ranking", "The words together in the title", "the weight of the words as a phrase in the title", 0.0, 3.0, 0.1),
    ("body", "Ranking", "The story's text", "the weight of BM25 of the text of Ask and Show HN stories", 0.0, 2.0, 0.1),
    ("link", "Ranking", "The link's words", "the weight of BM25 of the words of the link", 0.0, 2.0, 0.1),
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
            match key.as_str() {
                "understand" => k.understand = b()?,
                "meaning" => k.meaning = b()?,
                "typos" => k.typos = b()?,
                "synonyms" => k.synonyms = b()?,
                "exact" => k.exact = b()?,
                "tiers" => k.tiers = b()?,
                "rank" => {
                    let s = val.as_str().ok_or("rank is an ORDER BY expression")?.trim();
                    if !s.is_empty() {
                        k.rank = Some(rank_expression(s)?);
                    }
                }
                "max_per_site" => k.max_per_site = val.as_u64().filter(|n| *n <= 10).ok_or("max_per_site is 0 (any) to 10")? as usize,
                "fresh" => k.fresh = val.as_u64().filter(|n| *n <= 5).ok_or("fresh is 0 to 5 places")? as usize,
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
                    match other {
                        "votes" => k.votes = x,
                        "recency" => k.recency = x,
                        "meaning_weight" => k.meaning_weight = x,
                        "phrase" => k.phrase = x,
                        "body" => k.body = x,
                        _ => k.link = x,
                    }
                }
            }
        }
        Ok(k)
    }

    /// The options of a BM25 or MATCH of the words: typos and synonyms.
    pub fn word_opts(&self) -> String {
        format!("{}{}", if self.typos { ", fuzzy => 'auto'" } else { "" }, if self.synonyms { ", synonyms => 1" } else { "" })
    }

    /// The ranking of the candidates: the knobs' (which, at the defaults,
    /// is the site's), or the arm's own expression.
    pub fn ranking(&self, meaning: bool) -> String {
        if let Some(r) = &self.rank {
            return format!("({r})");
        }
        let o = self.word_opts();
        let sim = if meaning { format!(" + {} * coalesce(similarity(emb, :v), 0)", num(self.meaning_weight)) } else { String::new() };
        format!(
            "(coalesce(bm25(title, :q{o}), 0) + {} * coalesce(bm25(title, :phrase), 0) + {} * coalesce(bm25(body, :q{o}), 0) + {} * coalesce(bm25(urlwords, :q), 0){sim}) * pow(points + 1, {}) * ({} + {} * decay(created, now(), 157680000))",
            num(self.phrase),
            num(self.body),
            num(self.link),
            num(self.votes),
            num(1.0 - self.recency),
            num(self.recency)
        )
    }

    /// Blending after ranking: at most `max_per_site` stories of one site,
    /// and `fresh` places for the best of the last year; the order within
    /// each kept as ranked. `site` and `created` (unix seconds) of each row.
    pub fn blend<T>(&self, rows: Vec<T>, site: impl Fn(&T) -> String, created: impl Fn(&T) -> i64, now: i64, page: usize) -> Vec<T> {
        let mut rows = rows;
        if self.max_per_site > 0 {
            let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let (mut keep, mut later) = (Vec::new(), Vec::new());
            for r in rows {
                let s = site(&r);
                let n = seen.entry(s.clone()).or_insert(0);
                if s.is_empty() || *n < self.max_per_site {
                    *n += 1;
                    keep.push(r);
                } else {
                    later.push(r);
                }
            }
            keep.extend(later);
            rows = keep;
        }
        if self.fresh > 0 {
            let year = now - 365 * 86_400;
            let fresh_at: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| created(r) >= year).map(|(i, _)| i).take(self.fresh).collect();
            let shown = page.min(rows.len());
            let already = fresh_at.iter().filter(|&&i| i < shown).count();
            let missing: Vec<usize> = fresh_at.into_iter().filter(|&i| i >= shown).take(self.fresh.saturating_sub(already)).collect();
            if !missing.is_empty() {
                // The fresh ones take the page's last places, in their order.
                let mut taken = Vec::new();
                for &i in missing.iter().rev() {
                    taken.push(rows.remove(i));
                }
                taken.reverse();
                let at = shown.saturating_sub(taken.len()).min(rows.len());
                for (j, r) in taken.into_iter().enumerate() {
                    rows.insert(at + j, r);
                }
            }
        }
        rows
    }
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
    let d = Knobs::default();
    let mut knobs = vec![
        json!({"key": "understand", "group": "Query understanding", "title": "Read filters from the words", "about": "“by pg”, “in 2015”, “ask hn”, “points>100” and questions in a sentence become filters and sorts; off, every word is searched as typed", "kind": "bool", "default": d.understand}),
        json!({"key": "typos", "group": "Retrieval", "title": "Typos", "about": "words one or two letters away match too (fuzzy)", "kind": "bool", "default": d.typos}),
        json!({"key": "synonyms", "group": "Retrieval", "title": "Synonyms", "about": "a word's synonyms match too (js and javascript)", "kind": "bool", "default": d.synonyms}),
        json!({"key": "meaning", "group": "Retrieval", "title": "Search by meaning", "about": "stories near the words in meaning are candidates, and meaning counts in the ranking (off: words alone)", "kind": "bool", "default": d.meaning}),
        json!({"key": "exact", "group": "Retrieval", "title": "Code as written", "about": "words that look like code (std::move, node.js) are also found exactly as written, by the substring index", "kind": "bool", "default": d.exact}),
    ];
    for &(key, group, title, about, min, max, step) in NUMBERS {
        let def = match key {
            "votes" => d.votes,
            "recency" => d.recency,
            "meaning_weight" => d.meaning_weight,
            "phrase" => d.phrase,
            "body" => d.body,
            _ => d.link,
        };
        knobs.push(json!({"key": key, "group": group, "title": title, "about": about, "kind": "number", "default": def, "min": min, "max": max, "step": step}));
    }
    knobs.push(json!({"key": "rank", "group": "Ranking", "title": "Ranking expression", "about": "replaces the ranking the knobs above make: an ORDER BY expression of ChloeDB's SQL over hn_stories (:q the words, :phrase them quoted, :v their meaning)", "kind": "sql", "default": d.ranking(true)}));
    knobs.push(json!({"key": "tiers", "group": "Ranking", "title": "Every word in the title first", "about": "stories with every word of a short query in the title (then with a typo) come before the rest, as people expect of a search box", "kind": "bool", "default": d.tiers}));
    knobs.push(json!({"key": "max_per_site", "group": "Blending", "title": "At most this many of one site", "about": "after ranking, a site's stories past this many in the page move below the others (0: any number)", "kind": "int", "default": 0, "min": 0, "max": 10}));
    knobs.push(json!({"key": "fresh", "group": "Blending", "title": "Places for the last year", "about": "after ranking, this many places of the page go to the best stories of the last year, if the ranking left them below", "kind": "int", "default": 0, "min": 0, "max": 5}));
    let mut index_choices = vec![json!({"value": "main", "title": "hn_stories", "about": "the stories as the site indexes them"})];
    for (n, t) in indexes {
        index_choices.push(json!({"value": n, "title": t, "about": format!("{t}, an index of the stories built another way")}));
    }
    knobs.push(json!({"key": "index", "group": "Index", "title": "Index", "about": "the table of stories the search reads: an index built with other analyzers, fields or embeddings, beside the main one", "kind": "choice", "default": "main", "choices": index_choices}));
    json!({
        "protocol": 1,
        "site": "hn",
        "title": "Hacker News",
        "about": "Hacker News: every story since 2006 with its points, comments, link and text, searched by its title, text, link and meaning. People come to find a discussion they remember, what HN thinks of a tool, company or idea, Show HN launches, or the best stories on a subject.",
        "rows": "stories",
        "knobs": knobs,
        "presets": [
            {"name": "As it is", "knobs": {}},
            {"name": "Words only", "knobs": {"meaning": false}},
            {"name": "No typos or synonyms", "knobs": {"typos": false, "synonyms": false}},
            {"name": "Votes count less", "knobs": {"votes": 0.15}},
            {"name": "Newer first", "knobs": {"recency": 0.5}},
            {"name": "Words as typed", "knobs": {"understand": false}},
            {"name": "Two per site", "knobs": {"max_per_site": 2}},
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
            "(coalesce(bm25(title, :q, fuzzy => 'auto', synonyms => 1), 0) + 0.8 * coalesce(bm25(title, :phrase), 0) + 0.2 * coalesce(bm25(body, :q, fuzzy => 'auto', synonyms => 1), 0) + 0.3 * coalesce(bm25(urlwords, :q), 0) + 2.5 * coalesce(similarity(emb, :v), 0)) * pow(points + 1, 0.35) * (0.8 + 0.2 * decay(created, now(), 157680000))"
        );
        assert_eq!(k.word_opts(), ", fuzzy => 'auto', synonyms => 1");
    }

    #[test]
    fn knobs_are_checked() {
        let ix = vec![("v2".to_string(), "hn_stories_v2".to_string())];
        let k = Knobs::from_json(&json!({"meaning": false, "votes": 0.1, "typos": false, "index": "v2", "max_per_site": 2}), &ix).unwrap();
        assert!(!k.meaning && !k.typos && k.synonyms && k.index == "hn_stories_v2" && k.max_per_site == 2);
        assert!(k.ranking(false).contains("pow(points + 1, 0.1)") && !k.ranking(false).contains("fuzzy"));
        for bad in [json!({"votes": 3}), json!({"nope": 1}), json!({"meaning": 1}), json!({"index": "v3"}), json!({"rank": "points; DROP TABLE x"}), json!({"rank": "(SELECT 1)"}), json!({"rank": "points)"})] {
            assert!(Knobs::from_json(&bad, &ix).is_err(), "{bad}");
        }
        let k = Knobs::from_json(&json!({"rank": "bm25(title, :q) * log(points + 2)"}), &ix).unwrap();
        assert_eq!(k.ranking(true), "(bm25(title, :q) * log(points + 2))");
    }

    #[test]
    fn blending_moves_a_site_down_and_keeps_places_for_the_year() {
        let now = 400 * 86_400;
        let rows: Vec<(String, i64)> = vec![("a".into(), 0), ("a".into(), 0), ("a".into(), 0), ("b".into(), 0), ("c".into(), now)];
        let k = Knobs { max_per_site: 2, ..Knobs::default() };
        let out = k.blend(rows.clone(), |r| r.0.clone(), |r| r.1, now, 3);
        assert_eq!(out.iter().map(|r| r.0.as_str()).collect::<String>(), "aabca");
        let k = Knobs { fresh: 1, ..Knobs::default() };
        let out = k.blend(rows, |r| r.0.clone(), |r| r.1, now, 3);
        assert_eq!(out.iter().map(|r| r.0.as_str()).collect::<String>(), "aacab");
    }
}
