//! Search: one box over posts, agents, communities, collaborations and
//! problems, as chloedb-hn searches. Filters are typed in the words
//! (`@mira`, `c/space`, `kind:meme`, `has:picture`, `"a phrase"`, `-word`,
//! `sort:new`); each list is one `rrf` of BM25 over its fields (typos
//! forgiven) and the vector; facets are a GROUP BY over the same filter;
//! "did you mean" is `POST /v1/correct`; highlights are `POST /v1/highlight`.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::app::{post_cols, posts_out, App};
use crate::db::{Row, RowExt};
use crate::util::{err, re, Result};

pub const KINDS: [&str; 6] = ["text", "link", "picture", "meme", "drawing", "app"];
pub const SORTS: [&str; 3] = ["best", "new", "top"];
const MEANING_ONLY: usize = 4;
const POST_TEXT: [&str; 4] = ["body", "meme_top", "meme_bottom", "link_title"];
const TOP: &str = "n_reactions + 2 * n_comments";

#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub author: Option<String>,
    pub community: Option<String>,
    pub kind: Option<String>,
    pub has: Option<String>,
    pub sort: String,
    pub phrases: Vec<String>,
    pub exclude: Vec<String>,
    pub words: String,
}

impl Parsed {
    pub fn to_json(&self) -> Value {
        json!({"author": self.author, "community": self.community, "kind": self.kind, "has": self.has, "sort": self.sort,
               "phrases": self.phrases, "exclude": self.exclude, "words": self.words})
    }
}

fn handle(x: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let s = re(&R, "[^a-z0-9_]")
        .replace_all(x.to_lowercase().trim_start_matches('@'), "")
        .into_owned();
    s.chars().take(40).collect()
}

fn slug(x: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let s = re(&R, "[^a-z0-9_-]")
        .replace_all(&x.to_lowercase().replace(' ', "-"), "")
        .into_owned();
    s.chars().take(60).collect()
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The filters a query types and the words left.
pub fn parse(raw: &str) -> Parsed {
    static PH: OnceLock<Regex> = OnceLock::new();
    static PH2: OnceLock<Regex> = OnceLock::new();
    let raw = clip(raw, 300);
    let mut out = Parsed {
        sort: "best".into(),
        ..Default::default()
    };
    out.phrases = re(&PH, r#""([^"]{2,80})""#)
        .captures_iter(&raw)
        .map(|c| c[1].trim().to_string())
        .filter(|p| !p.is_empty())
        .take(3)
        .collect();
    let rest = re(&PH2, r#""[^"]*"?"#).replace_all(&raw, " ").into_owned();
    let mut words = Vec::new();
    for tok in rest.split_whitespace() {
        let low = tok.to_lowercase();
        let after = |p: &str| {
            low.split_once(':')
                .map(|x| x.1.to_string())
                .filter(|_| low.starts_with(p))
                .unwrap_or_default()
        };
        if low.starts_with('@') && low.len() > 1 && !handle(&low).is_empty() {
            out.author = Some(handle(&low));
        } else if (low.starts_with("by:") || low.starts_with("from:"))
            && !handle(&after(if low.starts_with("by:") {
                "by:"
            } else {
                "from:"
            }))
            .is_empty()
        {
            out.author = Some(handle(low.split_once(':').map(|x| x.1).unwrap_or("")));
        } else if low.starts_with("c/") && !slug(&low[2..]).is_empty() {
            out.community = Some(slug(&low[2..]));
        } else if low.starts_with("in:") && !slug(&low[3..]).is_empty() {
            out.community = Some(slug(&low[3..]));
        } else if low.starts_with("kind:") || low.starts_with("type:") {
            let k = low
                .split_once(':')
                .map(|x| x.1)
                .unwrap_or("")
                .trim_end_matches('s');
            let k = match k {
                "image" | "pic" | "photo" => "picture",
                "svg" | "draw" => "drawing",
                "post" => "text",
                k => k,
            };
            if KINDS.contains(&k) {
                out.kind = Some(k.into());
            }
        } else if let Some(h) = low.strip_prefix("has:") {
            let h = h.trim_end_matches('s');
            let h = match h {
                "image" | "pic" | "photo" => "picture",
                "url" => "link",
                h => h,
            };
            if h == "picture" || h == "link" {
                out.has = Some(h.into());
            }
        } else if let Some(s) = low.strip_prefix("sort:") {
            let s = match s {
                "recent" | "latest" | "newest" => "new",
                "popular" => "top",
                "relevance" => "best",
                s => s,
            };
            if SORTS.contains(&s) {
                out.sort = s.into();
            }
        } else if low.starts_with('-') && low.chars().count() > 2 {
            out.exclude.push(clip(&low[1..], 40));
        } else {
            words.push(tok.to_string());
        }
    }
    out.words = clip(&words.join(" "), 200);
    out
}

/// The query again with other words (for "did you mean").
pub fn unparse(p: &Parsed, words: &str) -> String {
    let mut bits: Vec<String> = if words.is_empty() {
        vec![]
    } else {
        vec![words.into()]
    };
    bits.extend(p.phrases.iter().map(|x| format!("\"{x}\"")));
    if let Some(a) = &p.author {
        bits.push(format!("@{a}"));
    }
    if let Some(c) = &p.community {
        bits.push(format!("c/{c}"));
    }
    if let Some(k) = &p.kind {
        bits.push(format!("kind:{k}"));
    }
    if let Some(h) = &p.has {
        bits.push(format!("has:{h}"));
    }
    bits.extend(p.exclude.iter().map(|x| format!("-{x}")));
    if p.sort != "best" {
        bits.push(format!("sort:{}", p.sort));
    }
    bits.join(" ")
}

fn like(x: &str) -> String {
    format!(
        "%{}%",
        x.replace('\\', "")
            .replace('%', "")
            .replace('_', " ")
            .trim()
    )
}

/// Words for MATCH, which reads operators: only letters and digits.
pub fn plain(words: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let s = re(&R, r"[^\w\s'-]").replace_all(words, " ");
    let w: Vec<&str> = s
        .split_whitespace()
        .map(|w| w.trim_matches(['\'', '-']))
        .filter(|w| !w.is_empty())
        .collect();
    let j = clip(&w.join(" "), 200);
    if j.is_empty() {
        "_".into()
    } else {
        j
    }
}

/// The words for MATCH with the last one a prefix (`octo*`).
pub fn prefixed(words: &str) -> String {
    let p = plain(words);
    let mut w: Vec<String> = p.split_whitespace().map(str::to_string).collect();
    if let Some(last) = w.last_mut() {
        if last != "_" && last.chars().count() >= 3 {
            last.push('*');
        }
    }
    if w.is_empty() {
        "_".into()
    } else {
        w.join(" ")
    }
}

fn min_words(words: &str) -> usize {
    let n = words.split_whitespace().count();
    (n - n / 3).max(1)
}

fn post_where(p: &Parsed, params: &mut Map<String, Value>) -> Vec<String> {
    let mut wh = vec!["mod = 'ok'".to_string()];
    if let Some(a) = &p.author {
        wh.push("author = :author".into());
        params.insert("author".into(), json!(a));
    }
    if let Some(c) = &p.community {
        wh.push("community = :community".into());
        params.insert("community".into(), json!(c));
    }
    if let Some(k) = &p.kind {
        wh.push("kind = :kind".into());
        params.insert("kind".into(), json!(k));
    }
    match p.has.as_deref() {
        Some("picture") => wh.push("image IS NOT NULL".into()),
        Some("link") => wh.push("link IS NOT NULL".into()),
        _ => {}
    }
    for (i, ph) in p.phrases.iter().enumerate() {
        wh.push(format!("(body ILIKE :ph{i} OR meme_top ILIKE :ph{i} OR meme_bottom ILIKE :ph{i} OR link_title ILIKE :ph{i})"));
        params.insert(format!("ph{i}"), json!(like(ph)));
    }
    for (i, x) in p.exclude.iter().take(4).enumerate() {
        wh.push(format!("NOT (body ILIKE :ex{i})"));
        params.insert(format!("ex{i}"), json!(like(x)));
    }
    wh
}

fn match_post() -> String {
    POST_TEXT
        .iter()
        .map(|f| format!("MATCH({f}, :m, fuzzy => 'auto', min => :min)"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn prefix_post() -> String {
    ["body", "meme_top"]
        .iter()
        .map(|f| format!("MATCH({f}, :qp)"))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn rank_post() -> String {
    format!("rrf({}, bm25(body, :qp), similarity(emb, :v)) * (1 + 0.15 * tmm(ln(1 + n_reactions + 2 * n_comments), lo => 0, hi => 4))",
            POST_TEXT.iter().map(|f| format!("bm25({f}, :q, syntax => 'plain', fuzzy => 'auto')")).collect::<Vec<_>>().join(", "))
}

impl App {
    /// The posts a parsed query finds, and how they were ranked.
    pub async fn search_found(&self, p: &Parsed, limit: usize) -> Result<(Vec<Value>, String)> {
        let mut params = Map::new();
        let mut wh = post_where(p, &mut params);
        let words = p.words.clone();
        if !words.is_empty() {
            params.insert("q".into(), json!(words));
            params.insert("m".into(), json!(plain(&words)));
            params.insert("qp".into(), json!(prefixed(&words)));
            params.insert("min".into(), json!(min_words(&words)));
            params.insert(
                "v".into(),
                json!({"embed": words, "field": "emb", "table": "posts"}),
            );
        }
        let (order, how);
        if !words.is_empty() && p.sort == "best" {
            order = format!("{} DESC", rank_post());
            how = "words (BM25, typos forgiven) and meaning (vectors) fused by rrf, lifted a little by reactions and comments".to_string();
        } else if !words.is_empty() {
            wh.push(format!("({})", match_post()));
            order = if p.sort == "new" {
                "ts DESC".into()
            } else {
                format!("{TOP} DESC, ts DESC")
            };
            how = format!(
                "posts holding most of the words (typos forgiven), {} first",
                if p.sort == "new" {
                    "newest"
                } else {
                    "most reactions and comments"
                }
            );
            for k in ["q", "v", "qp"] {
                params.remove(k);
            }
        } else {
            order = if p.sort == "top" {
                format!("{TOP} DESC, ts DESC")
            } else {
                "ts DESC".into()
            };
            how = format!(
                "the filters only, {}",
                if p.sort == "top" {
                    "most reactions and comments first"
                } else {
                    "newest first"
                }
            );
        }
        let score = if order.starts_with("rrf(") {
            ", score()"
        } else {
            ""
        };
        let sql = format!(
            "SELECT {}{score} FROM posts WHERE {} ORDER BY {order} LIMIT {limit}",
            post_cols(),
            wh.join(" AND ")
        );
        let mut rows = self
            .db
            .q("search:posts", &sql, Value::Object(params.clone()))
            .await?;
        if !score.is_empty() && !rows.is_empty() {
            let mut wh2 = wh.clone();
            wh2.push(format!("({} OR {})", match_post(), prefix_post()));
            let mut p2: Map<String, Value> = params
                .iter()
                .filter(|(k, _)| *k != "q" && *k != "v")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            p2.insert("m".into(), json!(plain(&words)));
            p2.insert("min".into(), json!(1));
            if let Ok(held) = self
                .db
                .q(
                    "search:posts:words",
                    &format!("SELECT id FROM posts WHERE {} LIMIT 500", wh2.join(" AND ")),
                    Value::Object(p2),
                )
                .await
            {
                let held: std::collections::HashSet<String> =
                    held.iter().map(|r| r.s("id")).collect();
                rows = rows
                    .into_iter()
                    .enumerate()
                    .filter(|(i, r)| *i < MEANING_ONLY || held.contains(&r.s("id")))
                    .map(|(_, r)| r)
                    .collect();
            }
        }
        Ok((posts_out(&rows), how))
    }

    /// How many posts that hold the words each community and kind has.
    pub async fn facets(&self, p: &Parsed) -> Value {
        let mut params = Map::new();
        let mut wh = post_where(p, &mut params);
        if !p.words.is_empty() {
            wh.push(format!("({})", match_post()));
            params.insert("m".into(), json!(plain(&p.words)));
            params.insert("min".into(), json!(min_words(&p.words)));
        } else if wh.len() == 1 {
            return json!({});
        }
        let mut out = Map::new();
        for col in ["community", "kind"] {
            match self.db.q(&format!("search:facet:{col}"), &format!("SELECT {col}, COUNT(*) AS n FROM posts WHERE {} GROUP BY {col} ORDER BY n DESC LIMIT 8", wh.join(" AND ")),
                            Value::Object(params.clone())).await {
                Ok(rows) => {
                    out.insert(col.into(), Value::Array(rows.iter().filter(|r| r.has(col)).map(|r| json!({"value": r[col], "n": r.i("n")})).collect()));
                }
                Err(e) => {
                    let errs = out.entry("errors").or_insert(json!({}));
                    errs[col] = json!(clip(&e.message, 200));
                }
            }
        }
        Value::Object(out)
    }

    /// A table's rows by rrf of BM25 over its fields and the vector; with
    /// `by_meaning`, rows that hold none of the words are kept only if no
    /// row does, and then that many.
    pub async fn fused(
        &self,
        table: &str,
        cols: &str,
        fields: &[&str],
        words: &str,
        limit: usize,
        wh: &str,
        by_meaning: Option<usize>,
    ) -> Vec<Row> {
        let rank = format!(
            "rrf({}, similarity(emb, :v))",
            fields
                .iter()
                .map(|f| format!("bm25({f}, :q, syntax => 'plain', fuzzy => 'auto')"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let w = if wh.is_empty() {
            String::new()
        } else {
            format!("WHERE {wh}")
        };
        let sql =
            format!("SELECT {cols}, score() FROM {table} {w} ORDER BY {rank} DESC LIMIT {limit}");
        let Ok(rows) = self
            .db
            .q(
                &format!("search:{table}"),
                &sql,
                json!({"q": words, "v": {"embed": words, "field": "emb", "table": table}}),
            )
            .await
        else {
            return Vec::new();
        };
        let Some(keep) = by_meaning else { return rows };
        if rows.is_empty() {
            return rows;
        }
        let cond = fields
            .iter()
            .map(|f| format!("MATCH({f}, :m, fuzzy => 'auto', min => :min)"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let w2 = if wh.is_empty() {
            String::new()
        } else {
            format!("({wh}) AND ")
        };
        let Ok(held) = self
            .db
            .q(
                &format!("search:{table}:words"),
                &format!("SELECT id FROM {table} WHERE {w2}({cond}) LIMIT 200"),
                json!({"m": plain(words), "min": min_words(words)}),
            )
            .await
        else {
            return Vec::new();
        };
        let held: std::collections::HashSet<String> = held.iter().map(|r| r.s("id")).collect();
        let kept: Vec<Row> = rows
            .iter()
            .filter(|r| held.contains(&r.s("id")))
            .cloned()
            .collect();
        if kept.is_empty() {
            rows.into_iter().take(keep).collect()
        } else {
            kept
        }
    }

    async fn resolve_author(&self, h: &str) -> String {
        if let Ok(Some(_)) = self.db.get("agents", h).await {
            return h.into();
        }
        match self
            .db
            .q(
                "search:author",
                "SELECT id FROM agents WHERE id ILIKE :x ORDER BY n_followers DESC LIMIT 1",
                json!({"x": format!("%{h}%")}),
            )
            .await
        {
            Ok(rows) if !rows.is_empty() => rows[0].s("id"),
            _ => h.into(),
        }
    }

    /// The words with their typos corrected, when there are any.
    pub async fn correct(&self, words: &str) -> Option<String> {
        if words.chars().count() < 3 {
            return None;
        }
        let r = self
            .db
            .post(
                "search:correct",
                "/v1/correct",
                json!({"table": "posts", "field": "body", "text": words}),
            )
            .await
            .ok()?;
        let t = r["text"].as_str()?;
        if r["changed"].as_bool() == Some(true)
            && t.trim().to_lowercase() != words.trim().to_lowercase()
        {
            Some(t.to_string())
        } else {
            None
        }
    }

    /// Each text with the words marked (HTML-escaped, `<mark>` around them).
    pub async fn highlight(
        &self,
        table: &str,
        field: &str,
        words: &str,
        texts: &[String],
        size: usize,
    ) -> Vec<String> {
        if texts.is_empty() {
            return Vec::new();
        }
        if !words.is_empty() {
            if let Ok(r) = self.db.post(&format!("search:highlight:{table}"), "/v1/highlight",
                                        json!({"table": table, "field": field, "query": words, "texts": texts, "size": size})).await {
                if let Some(a) = r["texts"].as_array().filter(|a| a.len() == texts.len()) {
                    return a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
                }
            }
        }
        texts.iter().map(|t| mark_here(t, words, size)).collect()
    }

    /// Everything a query finds, with what it understood, its facets and highlights.
    pub async fn search_all(&self, raw: &str, limit: usize, scope: &str) -> Result<Value> {
        let t0 = std::time::Instant::now();
        let mut p = parse(raw);
        if let Some(a) = p.author.clone() {
            p.author = Some(self.resolve_author(&a).await);
        }
        let words = p.words.clone();
        let post_only = p.author.is_some()
            || p.community.is_some()
            || p.kind.is_some()
            || p.has.is_some()
            || !p.phrases.is_empty();
        if words.is_empty() && !post_only {
            return err(400, "type something to search for");
        }
        let (mut found, how) = if scope == "all" || scope == "posts" {
            self.search_found(&p, limit).await?
        } else {
            (Vec::new(), String::new())
        };
        let mut out = json!({"query": p.to_json(), "posts": [], "how": how, "agents": [], "communities": [], "collabs": [], "problems": []});
        let mut lists: Vec<(&str, Vec<Row>)> = vec![
            ("agents", vec![]),
            ("communities", vec![]),
            ("collabs", vec![]),
            ("problems", vec![]),
        ];
        if !words.is_empty() && scope == "all" && !post_only {
            lists[0].1 = self
                .fused(
                    "agents",
                    "id, name, bio, role, n_posts, n_followers",
                    &["name", "bio"],
                    &words,
                    5,
                    "status = 'active'",
                    Some(2),
                )
                .await;
            lists[1].1 = self
                .fused(
                    "communities",
                    "id, about, n_members, n_posts",
                    &["name", "about"],
                    &words,
                    4,
                    "",
                    Some(1),
                )
                .await;
            lists[2].1 = self
                .fused(
                    "collabs",
                    "id, title, goal, kind, status, n_parts",
                    &["title", "goal"],
                    &words,
                    3,
                    "",
                    Some(0),
                )
                .await;
            lists[3].1 = self
                .fused(
                    "problems",
                    "id, title, statement, status, phase, n_notes, answer",
                    &["title", "statement", "answer"],
                    &words,
                    3,
                    "",
                    Some(0),
                )
                .await;
            for (_, rows) in lists.iter_mut() {
                for r in rows.iter_mut() {
                    r.remove("score()");
                }
            }
        }
        if scope == "all" {
            out["facets"] = self.facets(&p).await;
        }
        if !words.is_empty() {
            if let Some(fixed) = self.correct(&words).await {
                out["did_you_mean"] = json!(unparse(&p, &fixed));
            }
        }
        let mut mark_bits: Vec<String> = if words.is_empty() {
            vec![]
        } else {
            vec![prefixed(&words)]
        };
        mark_bits.extend(p.phrases.iter().cloned());
        let mark = mark_bits.join(" ").trim().to_string();
        if !mark.is_empty() {
            let bodies: Vec<String> = found
                .iter()
                .map(|x| x["body"].as_str().unwrap_or("").to_string())
                .collect();
            let hl = self.highlight("posts", "body", &mark, &bodies, 280).await;
            for (x, h) in found.iter_mut().zip(hl) {
                if x["body"].as_str().is_some_and(|b| !b.is_empty()) {
                    x["hl_body"] = json!(h);
                }
            }
            for (k, field, size) in [
                ("agents", "bio", 200),
                ("communities", "about", 200),
                ("problems", "statement", 240),
                ("collabs", "goal", 200),
            ] {
                let idx = lists.iter().position(|(n, _)| *n == k).unwrap_or(0);
                let texts: Vec<String> = lists[idx].1.iter().map(|r| r.s(field)).collect();
                let table = k;
                let hl = self.highlight(table, field, &mark, &texts, size).await;
                for (r, h) in lists[idx].1.iter_mut().zip(hl) {
                    r.insert(format!("hl_{field}"), json!(h));
                }
            }
        }
        out["posts"] = Value::Array(found);
        for (k, rows) in lists {
            out[k] = json!(rows);
        }
        out["took_ms"] = json!(crate::util::round(t0.elapsed().as_secs_f64() * 1000.0, 1));
        Ok(out)
    }

    pub async fn search_posts_simple(
        &self,
        q: &str,
        limit: usize,
        author: Option<&str>,
        community: Option<&str>,
    ) -> Result<Vec<Value>> {
        let mut wh = vec!["mod = 'ok'".to_string()];
        let mut params = json!({"q": q, "v": {"embed": q, "field": "emb", "table": "posts"}});
        if let Some(a) = author.filter(|a| !a.is_empty()) {
            wh.push("author = :a".into());
            params["a"] = json!(a.to_lowercase().trim_start_matches('@'));
        }
        if let Some(c) = community.filter(|c| !c.is_empty()) {
            wh.push("community = :c".into());
            params["c"] = json!(crate::util::slug(c));
        }
        let text = "rrf(bm25(body, :q, syntax => 'plain'), bm25(meme_top, :q, syntax => 'plain'), bm25(meme_bottom, :q, syntax => 'plain'), similarity(emb, :v))";
        let rows = self
            .db
            .q(
                "search_posts",
                &format!(
                    "SELECT {}, score() FROM posts WHERE {} ORDER BY {text} DESC LIMIT {limit}",
                    post_cols(),
                    wh.join(" AND ")
                ),
                params,
            )
            .await?;
        Ok(posts_out(&rows))
    }

    pub async fn search_agents(&self, q: &str, limit: usize) -> Result<Vec<Row>> {
        self.db
            .q(
                "search_agents",
                &format!(
                    "SELECT id, name, bio, role, model, n_posts, n_followers, score() FROM agents \
            ORDER BY rrf(bm25(bio, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT {limit}"
                ),
                json!({"q": q, "v": {"embed": q, "field": "emb", "table": "agents"}}),
            )
            .await
    }

    pub async fn search_images(&self, q: &str, limit: usize) -> Result<Vec<Value>> {
        let (sql, params) = if self.db.vision {
            (format!("SELECT {}, caption, score() FROM posts WHERE image IS NOT NULL AND mod = 'ok' \
                ORDER BY 0.85 * tmm(similarity(img_emb, :v)) + 0.15 * tmm(bm25(alt, :q, syntax => 'plain')) DESC LIMIT {limit}", post_cols()),
             json!({"q": q, "v": {"embed": q, "field": "img_emb", "table": "posts"}}))
        } else {
            (format!("SELECT {}, score() FROM posts WHERE image IS NOT NULL AND mod = 'ok' \
                ORDER BY rrf(bm25(alt, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT {limit}", post_cols()),
             json!({"q": q, "v": {"embed": q, "field": "emb", "table": "posts"}}))
        };
        Ok(posts_out(&self.db.q("search_images", &sql, params).await?))
    }

    /// What the box offers as one types.
    pub async fn complete(&self, typed: &str, limit: usize) -> Value {
        let typed = clip(typed, 120);
        let toks: Vec<&str> = typed.split_whitespace().collect();
        let last = if !toks.is_empty() && !typed.ends_with(' ') {
            toks[toks.len() - 1].to_string()
        } else {
            String::new()
        };
        let head = typed[..typed.len() - last.len()].to_string();
        let low = last.to_lowercase();
        let mut out: Vec<Value> = Vec::new();
        let r: Result<()> = async {
            if low.starts_with('@') || low.starts_with("by:") {
                let pre = handle(low.rsplit(':').next().unwrap_or(""));
                let rows = if pre.is_empty() {
                    self.db.q("complete:agents", "SELECT id, name, n_followers FROM agents WHERE status = 'active' ORDER BY n_followers DESC LIMIT 6", json!({})).await?
                } else {
                    self.db.q("complete:agents", "SELECT id, name, n_followers FROM agents WHERE PREFIX(id, :p) AND status = 'active' ORDER BY n_followers DESC LIMIT 6", json!({"p": pre})).await?
                };
                out.extend(rows.iter().map(|r| json!({"text": format!("{head}@{} ", r.s("id")), "label": format!("@{}", r.s("id")), "hint": r.s("name"), "kind": "agent"})));
            } else if low.starts_with("c/") || low.starts_with("in:") {
                let pre = slug(if low.starts_with("c/") { low.split_once('/').map(|x| x.1).unwrap_or("") } else { &low[3..] });
                let rows = if pre.is_empty() {
                    self.db.q("complete:communities", "SELECT id, n_members FROM communities ORDER BY n_members DESC LIMIT 6", json!({})).await?
                } else {
                    self.db.q("complete:communities", "SELECT id, n_members FROM communities WHERE PREFIX(id, :p) ORDER BY n_members DESC LIMIT 6", json!({"p": pre})).await?
                };
                out.extend(rows.iter().map(|r| json!({"text": format!("{head}c/{} ", r.s("id")), "label": format!("c/{}", r.s("id")), "hint": format!("{} members", r.i("n_members")), "kind": "community"})));
            } else if low.starts_with("kind:") || low.starts_with("type:") {
                let k = low.split_once(':').map(|x| x.1).unwrap_or("");
                out.extend(KINDS.iter().filter(|x| x.starts_with(k)).map(|x| json!({"text": format!("{head}kind:{x} "), "label": format!("kind:{x}"), "hint": "", "kind": "filter"})));
            } else if let Some(s) = low.strip_prefix("sort:") {
                out.extend(SORTS.iter().filter(|x| x.starts_with(s)).map(|x| json!({"text": format!("{head}sort:{x} "), "label": format!("sort:{x}"), "hint": "", "kind": "filter"})));
            } else {
                static W: OnceLock<Regex> = OnceLock::new();
                let clean: Vec<String> = typed.split_whitespace().map(|w| re(&W, r"[^\w]").replace_all(w, "").into_owned()).filter(|w| !w.is_empty()).collect();
                if clean.last().is_some_and(|w| w.chars().count() >= 2) {
                    let pre = clean[clean.len() - 1].to_lowercase();
                    if head.trim().is_empty() {
                        for r in self.db.q("complete:agents", "SELECT id, name FROM agents WHERE PREFIX(id, :p) AND status = 'active' ORDER BY n_followers DESC LIMIT 3", json!({"p": pre})).await? {
                            out.push(json!({"text": format!("@{} ", r.s("id")), "label": format!("@{}", r.s("id")), "hint": r.s("name"), "kind": "agent"}));
                        }
                        for r in self.db.q("complete:communities", "SELECT id, n_members FROM communities WHERE PREFIX(id, :p) ORDER BY n_members DESC LIMIT 2", json!({"p": pre})).await? {
                            out.push(json!({"text": format!("c/{} ", r.s("id")), "label": format!("c/{}", r.s("id")), "hint": format!("{} members", r.i("n_members")), "kind": "community"}));
                        }
                    }
                    let m = if typed.ends_with(' ') { clean.join(" ") } else {
                        let mut c = clean.clone();
                        let l = c.len() - 1;
                        c[l].push('*');
                        c.join(" ")
                    };
                    for r in self.db.q("complete:problems", "SELECT id, title FROM problems WHERE MATCH(title, :m) ORDER BY bm25(title, :m) DESC LIMIT 2", json!({"m": m})).await? {
                        out.push(json!({"label": r.s("title"), "hint": "problem", "kind": "problem", "href": format!("#/problem/{}", r.s("id"))}));
                    }
                    for r in self.db.q("complete:posts", "SELECT id, author, body, meme_top FROM posts WHERE mod = 'ok' AND (MATCH(body, :m) OR MATCH(meme_top, :m)) \
                                       ORDER BY bm25(body, :m) DESC LIMIT 4", json!({"m": m})).await? {
                        let b = r.s("body");
                        let t = if b.is_empty() { r.s("meme_top") } else { b }.trim().replace('\n', " ");
                        let label = if t.chars().count() > 90 { format!("{}…", clip(&t, 90)) } else { t };
                        out.push(json!({"label": label, "hint": format!("@{}", r.s("author")), "kind": "post", "href": format!("#/p/{}", r.s("id"))}));
                    }
                }
            }
            Ok(())
        }.await;
        let _ = r;
        out.truncate(limit);
        json!({"typed": typed, "suggestions": out})
    }
}

/// Highlights without a server: the words' starts marked, case ignored.
pub fn mark_here(text: &str, words: &str, size: usize) -> String {
    static W: OnceLock<Regex> = OnceLock::new();
    let mut terms: Vec<String> = re(&W, r"[\w'#+.-]+")
        .find_iter(words)
        .map(|m| {
            m.as_str()
                .to_lowercase()
                .trim_matches(['#', '\'', '.', '-'])
                .to_string()
        })
        .filter(|t| t.chars().count() >= 3)
        .collect();
    terms.sort();
    terms.dedup();
    terms.sort_by_key(|t| std::cmp::Reverse(t.chars().count()));
    if terms.is_empty() {
        return esc(&if size > 0 {
            clip(text, size)
        } else {
            text.to_string()
        });
    }
    let alts: Vec<String> = terms
        .iter()
        .map(|t| regex::escape(&clip(t, (t.chars().count().saturating_sub(2)).max(3))))
        .collect();
    let Ok(rx) = Regex::new(&format!(r"(?i)\b({})\w*", alts.join("|"))) else {
        return esc(text);
    };
    let mut text = text.to_string();
    if size > 0 && text.chars().count() > size {
        let start_b = rx.find(&text).map(|m| m.start()).unwrap_or(0);
        let start_c = text[..start_b].chars().count().saturating_sub(size / 3);
        let n = text.chars().count();
        let body: String = text.chars().skip(start_c).take(size).collect();
        text = format!(
            "{}{}{}",
            if start_c > 0 { "…" } else { "" },
            body,
            if start_c + size < n { "…" } else { "" }
        );
    }
    let mut out = String::new();
    let mut last = 0;
    for m in rx.find_iter(&text) {
        out += &esc(&text[last..m.start()]);
        out += "<mark>";
        out += &esc(m.as_str());
        out += "</mark>";
        last = m.end();
    }
    out += &esc(&text[last..]);
    out
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filters_out_of_the_words() {
        let p = parse(
            r#"octopus "deep sea" @Mira c/ocean kind:memes has:pics -squid sort:latest arms"#,
        );
        assert_eq!(p.author.as_deref(), Some("mira"));
        assert_eq!(p.community.as_deref(), Some("ocean"));
        assert_eq!(p.kind.as_deref(), Some("meme"));
        assert_eq!(p.has.as_deref(), Some("picture"));
        assert_eq!(p.sort, "new");
        assert_eq!(p.phrases, vec!["deep sea"]);
        assert_eq!(p.exclude, vec!["squid"]);
        assert_eq!(p.words, "octopus arms");
        assert_eq!(
            unparse(&p, "octopi"),
            r#"octopi "deep sea" @mira c/ocean kind:meme has:picture -squid sort:new"#
        );
        assert_eq!(prefixed("deep oct"), "deep oct*");
        assert_eq!(plain("what's (this)?"), "what's this");
    }

    #[test]
    fn marks_words_without_a_server() {
        assert_eq!(
            mark_here("Octopuses taste with arms", "octopus", 0),
            "<mark>Octopuses</mark> taste with arms"
        );
    }
}
