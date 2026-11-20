//! Real sites as sources of experiments: a site's own search, run as an
//! arm asks, on its real data. Nothing is written to a real site: an arm
//! only reads (a search, a row opened, rows to write personas from).
//!
//! Two kinds of source:
//!
//! - an **app** of ChloeDB that takes experiments (chloedb-hn,
//!   chloedb-arxiv started with `--experiments-token`): it says what may be
//!   changed of its search (`GET /v1/experiment`, its knobs in groups:
//!   query understanding, retrieval, ranking, blending, index), and runs
//!   each search with an arm's knobs on its own cluster;
//! - a **public search** of a site, for an arm of the site as anyone sees
//!   it, or of another engine on the same rows: Hacker News's and arXiv's
//!   public pages on ChloeDB (`hn-public`, `arxiv-public`), and Algolia's
//!   search of Hacker News (`algolia-hn`), whose stories have the same ids.
//!   Their knobs are the parameters their public search takes.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::server::Result;

/// The kinds of source, as `--source NAME=KIND:URL` names them.
pub const KINDS: [(&str, &str); 4] = [
    ("app", "an app of ChloeDB that takes experiments (its /v1/experiment, with its experiments token)"),
    ("hn-public", "Hacker News on ChloeDB, its public search (hn.chloetech.ai)"),
    ("arxiv-public", "arXiv on ChloeDB, its public search (arxiv.chloetech.ai)"),
    ("algolia-hn", "Algolia's search of Hacker News (hn.algolia.com/api/v1), the same stories by the same ids"),
];

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    App,
    HnPublic,
    ArxivPublic,
    AlgoliaHn,
}

/// A row of a results page.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub url: String,
}

/// A page of results, and what the search took.
#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub rows: Vec<Hit>,
    /// What the site read in the words (filters, sorts), as it says.
    pub understood: Value,
    pub ms: f64,
}

/// What a reader sees on opening a row.
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub url: String,
    pub text: String,
}

pub struct Source {
    pub name: String,
    pub kind: Kind,
    pub base: String,
    token: Option<String>,
    http: reqwest::Client,
    /// Requests in flight to the site, at most four: an arm is a few
    /// people, not load.
    gate: tokio::sync::Semaphore,
    described: tokio::sync::Mutex<Option<(Instant, Value)>>,
}

/// The environment variable of a source's experiments token.
pub fn token_var(name: &str) -> String {
    format!("CHLOEDB_SOURCE_{}_TOKEN", name.to_ascii_uppercase().replace(['-', '.'], "_"))
}

impl Source {
    /// A source from `NAME=KIND:URL` (or `NAME=URL`, an app), its token from
    /// [`token_var`].
    pub fn parse(spec: &str) -> std::result::Result<Source, String> {
        let (name, rest) = spec.split_once('=').ok_or_else(|| format!("--source {spec}: NAME=KIND:URL"))?;
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("--source {spec}: a name of letters, digits, - and _"));
        }
        let (kind, url) = if rest.starts_with("http://") || rest.starts_with("https://") {
            ("app", rest)
        } else {
            rest.split_once(':').ok_or_else(|| format!("--source {spec}: NAME=KIND:URL"))?
        };
        let kind = match kind {
            "app" => Kind::App,
            "hn-public" => Kind::HnPublic,
            "arxiv-public" => Kind::ArxivPublic,
            "algolia-hn" => Kind::AlgoliaHn,
            k => return Err(format!("--source {spec}: no kind {k:?} ({})", KINDS.iter().map(|k| k.0).collect::<Vec<_>>().join(", "))),
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(format!("--source {spec}: the URL is http:// or https://"));
        }
        let token = std::env::var(token_var(name)).ok().filter(|t| !t.trim().is_empty());
        Ok(Source::new(name, kind, url, token))
    }

    pub fn new(name: &str, kind: Kind, base: &str, token: Option<String>) -> Source {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(40)).connect_timeout(Duration::from_secs(10)).user_agent("chloedb-experiments").build().expect("an HTTP client");
        Source { name: name.into(), kind, base: base.trim_end_matches('/').into(), token, http, gate: tokio::sync::Semaphore::new(4), described: tokio::sync::Mutex::new(None) }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut req = self.http.get(format!("{}{path}", self.base)).query(query);
        if self.kind == Kind::App {
            if let Some(t) = &self.token {
                req = req.bearer_auth(t);
            }
        }
        self.send(req, path).await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let mut req = self.http.post(format!("{}{path}", self.base)).json(body);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        self.send(req, path).await
    }

    async fn send(&self, req: reqwest::RequestBuilder, path: &str) -> Result<Value> {
        let _turn = self.gate.acquire().await.map_err(|e| e.to_string())?;
        let mut wait = Duration::from_millis(500);
        for attempt in 0.. {
            let r = match req.try_clone().expect("a body in memory").send().await {
                Ok(r) => r,
                Err(e) if attempt < 3 && (e.is_connect() || e.is_timeout()) => {
                    tokio::time::sleep(wait).await;
                    wait *= 2;
                    continue;
                }
                Err(e) => return Err(format!("{}: {path}: {e}", self.name)),
            };
            let status = r.status().as_u16();
            let text = r.text().await.map_err(|e| format!("{}: {path}: {e}", self.name))?;
            if (200..300).contains(&status) {
                return serde_json::from_str(&text).map_err(|e| format!("{}: {path}: {e}", self.name));
            }
            if matches!(status, 429 | 502 | 503 | 504) && attempt < 4 {
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(10));
                continue;
            }
            let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let m = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(240).collect());
            return Err(format!("{} ({status}): {m}", self.name));
        }
        unreachable!()
    }

    /// The site, its knobs and presets, as `GET /v1/experiment` says them
    /// (made here for a public search); `allowed` false, with the reason,
    /// when an app takes no experiments or refuses the token. Kept a minute.
    pub async fn describe(&self) -> Value {
        if let Some((t, v)) = &*self.described.lock().await {
            if t.elapsed() < Duration::from_secs(60) {
                return v.clone();
            }
        }
        let mut v = match self.kind {
            Kind::App => match self.get("/v1/experiment", &[]).await {
                Ok(v) => v,
                Err(e) => json!({"allowed": false, "error": e, "title": self.name, "knobs": [], "presets": []}),
            },
            Kind::HnPublic => json!({
                "site": "hn", "title": "Hacker News (public search)", "rows": "stories", "allowed": true,
                "about": "Hacker News: every story since 2006 with its points, comments, link and text. People come to find a discussion they remember, what HN thinks of a tool, company or idea, Show HN launches, or the best stories on a subject.",
                "knobs": [
                    {"key": "mode", "group": "Retrieval", "title": "Search by", "about": "the public search's own modes", "kind": "choice", "default": "hybrid", "choices": [{"value": "hybrid", "title": "Words and meaning"}, {"value": "words", "title": "Words"}, {"value": "meaning", "title": "Meaning"}]},
                    {"key": "sort", "group": "Ranking", "title": "Order", "about": "the order the page offers", "kind": "choice", "default": "best", "choices": [{"value": "best", "title": "Best"}, {"value": "date", "title": "Newest"}, {"value": "points", "title": "Most points"}]},
                ],
                "presets": [{"name": "As anyone sees it", "knobs": {}}, {"name": "Words only", "knobs": {"mode": "words"}}, {"name": "Newest first", "knobs": {"sort": "date"}}],
            }),
            Kind::ArxivPublic => json!({
                "site": "arxiv", "title": "arXiv (public search)", "rows": "papers", "allowed": true,
                "about": "arXiv: every paper with its title, abstract, authors, categories and citations. People come to find a paper they half remember, the papers on a method or problem, what a person or lab wrote, surveys to start from, or the newest work on a subject.",
                "knobs": [
                    {"key": "mode", "group": "Retrieval", "title": "Search by", "about": "the public search's own modes", "kind": "choice", "default": "hybrid", "choices": [{"value": "hybrid", "title": "Words and meaning"}, {"value": "words", "title": "Words"}, {"value": "meaning", "title": "Meaning"}]},
                    {"key": "sort", "group": "Ranking", "title": "Order", "about": "the order the page offers", "kind": "choice", "default": "best", "choices": [{"value": "best", "title": "Best"}, {"value": "date", "title": "Newest"}, {"value": "cites", "title": "Most cited"}]},
                ],
                "presets": [{"name": "As anyone sees it", "knobs": {}}, {"name": "Words only", "knobs": {"mode": "words"}}],
            }),
            Kind::AlgoliaHn => json!({
                "site": "hn", "title": "Algolia's Hacker News search", "rows": "stories", "allowed": true,
                "about": "Hacker News, searched by Algolia (hn.algolia.com): the stories HN links to its own search box, by the same ids as Hacker News on ChloeDB.",
                "knobs": [
                    {"key": "by", "group": "Ranking", "title": "Order", "about": "Algolia's two endpoints: relevance then points, or newest first", "kind": "choice", "default": "relevance", "choices": [{"value": "relevance", "title": "Relevance"}, {"value": "date", "title": "Newest"}]},
                    {"key": "typos", "group": "Retrieval", "title": "Typos", "about": "Algolia's typo tolerance", "kind": "bool", "default": true},
                ],
                "presets": [{"name": "As hn.algolia.com", "knobs": {}}, {"name": "Newest first", "knobs": {"by": "date"}}],
            }),
        };
        v["source"] = json!(self.name);
        v["kind"] = json!(match self.kind {
            Kind::App => "app",
            Kind::HnPublic => "hn-public",
            Kind::ArxivPublic => "arxiv-public",
            Kind::AlgoliaHn => "algolia-hn",
        });
        v["base"] = json!(self.base);
        if v["allowed"].is_null() {
            v["allowed"] = json!(true);
        }
        *self.described.lock().await = Some((Instant::now(), v.clone()));
        v
    }

    /// The site's logged searches since `since` (seconds), with what people
    /// opened, when the site logs them (its description has `log`).
    pub async fn log(&self, since: i64, limit: usize) -> Result<Value> {
        if self.kind != Kind::App {
            return Err(format!("{}: a public search keeps no log", self.name));
        }
        self.get("/v1/experiment/log", &[("since", since.to_string()), ("limit", limit.to_string())]).await
    }

    /// Whether the site can interleave two arms on its real visitors'
    /// searches (its description has `interleave`).
    pub async fn interleaves(&self) -> bool {
        self.kind == Kind::App && self.describe().await["interleave"].is_object()
    }

    /// Starts an interleaving test of `a` (the control) against `b` on
    /// the site's real visitors, on `share` of the page's searches.
    pub async fn interleave_start(&self, id: &str, name: &str, a: &Value, b: &Value, share: f64) -> Result<Value> {
        if !self.interleaves().await {
            return Err(format!("{} cannot interleave its visitors' searches", self.name));
        }
        self.post("/v1/experiment/interleave", &json!({"id": id, "name": name, "a": a, "b": b, "share": share})).await
    }

    pub async fn interleave_stop(&self, id: &str) -> Result<Value> {
        self.post("/v1/experiment/interleave/stop", &json!({"id": id})).await
    }

    /// The test running on the site, if any.
    pub async fn interleave_running(&self) -> Result<Value> {
        self.get("/v1/experiment/interleave", &[]).await
    }

    /// A test's interleaved searches, with what people opened.
    pub async fn interleave_log(&self, id: &str) -> Result<Value> {
        self.get("/v1/experiment/interleave/log", &[("test", id.to_string()), ("limit", "10000".into())]).await
    }

    /// The first page of `q`, as an arm with these knobs sees it.
    pub async fn search(&self, q: &str, knobs: &Value, per: usize) -> Result<Page> {
        let t0 = Instant::now();
        let s = |k: &str, d: &str| knobs[k].as_str().unwrap_or(d).to_string();
        let ms = |t0: Instant| (t0.elapsed().as_secs_f64() * 10_000.0).round() / 10.0;
        match self.kind {
            Kind::App => {
                let v = self.post("/v1/experiment/search", &json!({"q": q, "knobs": knobs, "per": per})).await?;
                let rows = v["rows"].as_array().map(|a| a.iter().map(hit).collect()).unwrap_or_default();
                Ok(Page { rows, understood: v["understood"].clone(), ms: ms(t0) })
            }
            Kind::HnPublic => {
                let v = self.get("/v1/search", &[("q", q.to_string()), ("only", "story".into()), ("per", per.to_string()), ("facets", "0".into()), ("comments", "0".into()), ("mode", s("mode", "hybrid")), ("sort", s("sort", "best"))]).await?;
                let rows = v["stories"].as_array().map(|a| a.iter().map(hn_hit).collect()).unwrap_or_default();
                Ok(Page { rows, understood: v["understood"]["notes"].clone(), ms: ms(t0) })
            }
            Kind::ArxivPublic => {
                let v = self.get("/v1/search", &[("q", q.to_string()), ("per", per.to_string()), ("facets", "0".into()), ("mode", s("mode", "hybrid")), ("sort", s("sort", "best"))]).await?;
                let rows = v["papers"].as_array().map(|a| a.iter().map(paper_hit).collect()).unwrap_or_default();
                Ok(Page { rows, understood: v["understood"]["notes"].clone(), ms: ms(t0) })
            }
            Kind::AlgoliaHn => {
                let path = if s("by", "relevance") == "date" { "/search_by_date" } else { "/search" };
                let typos = knobs["typos"].as_bool().unwrap_or(true);
                let v = self.get(path, &[("query", q.to_string()), ("tags", "story".into()), ("hitsPerPage", per.to_string()), ("typoTolerance", typos.to_string())]).await?;
                let rows = v["hits"].as_array().map(|a| a.iter().map(algolia_hit).collect()).unwrap_or_default();
                Ok(Page { rows, understood: Value::Null, ms: ms(t0) })
            }
        }
    }

    /// A row, as a reader sees it on opening it.
    pub async fn item(&self, id: &str) -> Result<Item> {
        let clean: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-' | '_')).collect();
        match self.kind {
            Kind::App => {
                let v = self.get(&format!("/v1/experiment/item/{clean}"), &[]).await?;
                Ok(Item { id: clean, title: text(&v["title"]), url: text(&v["url"]), text: text(&v["text"]) })
            }
            Kind::HnPublic => {
                let v = self.get(&format!("/v1/item/{clean}"), &[]).await?;
                let st = &v["story"];
                let h = hn_hit(st);
                let mut t = format!("{}\n{}\n{}\n", h.title, h.url, h.snippet);
                let body = plain(&text(&st["body"]));
                if !body.is_empty() {
                    t.push_str(&format!("\n{}\n", clip(&body, 1500)));
                }
                let comments = v["comments"].as_array().cloned().unwrap_or_default();
                if !comments.is_empty() {
                    t.push_str("\nComments:\n");
                    for c in comments.iter().take(6) {
                        let b = plain(&text(if c["body"].is_null() { &c["text"] } else { &c["body"] }));
                        t.push_str(&format!("\n- {}: {}\n", text(&c["author"]), clip(&b, 420)));
                    }
                }
                Ok(Item { id: clean, title: h.title, url: h.url, text: t })
            }
            Kind::ArxivPublic => {
                let v = self.get(&format!("/v1/paper/{clean}"), &[]).await?;
                let p = &v["paper"];
                let h = paper_hit(p);
                Ok(Item { id: clean, title: h.title.clone(), url: h.url, text: format!("{}\n{}\n{}\n\n{}\n", h.title, text(&p["authors"]), h.snippet, text(&p["abstract"])) })
            }
            Kind::AlgoliaHn => {
                let v = self.get(&format!("/items/{clean}"), &[]).await?;
                let title = text(&v["title"]);
                let url = text(&v["url"]);
                let mut t = format!("{title}\n{url}\n{} points · by {}\n", v["points"].as_i64().unwrap_or(0), text(&v["author"]));
                let body = plain(&text(&v["text"]));
                if !body.is_empty() {
                    t.push_str(&format!("\n{}\n", clip(&body, 1500)));
                }
                let kids = v["children"].as_array().cloned().unwrap_or_default();
                if !kids.is_empty() {
                    t.push_str("\nComments:\n");
                    for c in kids.iter().filter(|c| c["text"].is_string()).take(6) {
                        t.push_str(&format!("\n- {}: {}\n", text(&c["author"]), clip(&plain(&text(&c["text"])), 420)));
                    }
                }
                Ok(Item { id: clean, title, url, text: t })
            }
        }
    }

    /// Rows of the site to write personas and their needs from: an app's
    /// own sample, or, for a public search, the first pages of a few broad
    /// searches the seed picks.
    pub async fn sample(&self, n: usize, seed: u64) -> Result<Vec<Value>> {
        if self.kind == Kind::App {
            let v = self.get("/v1/experiment/sample", &[("n", n.to_string()), ("seed", seed.to_string())]).await?;
            return Ok(v["rows"].as_array().cloned().unwrap_or_default());
        }
        let words: &[&str] = match self.kind {
            Kind::ArxivPublic => &["language models", "graph neural networks", "quantum error correction", "reinforcement learning", "black holes", "protein structure", "diffusion models", "privacy", "robotics", "survey", "dark matter", "recommendation"],
            _ => &["rust", "startup", "show hn", "ask hn", "postgres", "privacy", "apple", "bitcoin", "machine learning", "remote work", "linux", "climate", "hiring", "security breach", "javascript framework", "space"],
        };
        let mut out: Vec<Value> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let pick = (seed as usize) % words.len();
        for w in words.iter().cycle().skip(pick).take(6) {
            let p = self.search(w, &json!({}), (n / 6).clamp(3, 10)).await?;
            for r in p.rows {
                if seen.insert(r.id.clone()) {
                    out.push(json!({"id": r.id, "title": r.title, "snippet": r.snippet, "url": r.url}));
                }
            }
        }
        out.truncate(n);
        Ok(out)
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn hit(v: &Value) -> Hit {
    Hit { id: text(&v["id"]), title: text(&v["title"]), snippet: text(&v["snippet"]), url: text(&v["url"]) }
}

fn year(v: &Value) -> String {
    text(v).get(..4).unwrap_or("").to_string()
}

/// A story of Hacker News on ChloeDB's public search, as its page shows it.
fn hn_hit(v: &Value) -> Hit {
    let mut line = Vec::new();
    for x in [text(&v["site"]), format!("{} points", v["points"].as_i64().unwrap_or(0)), format!("{} comments", v["comments"].as_i64().unwrap_or(0)), year(&v["created"]), format!("by {}", text(&v["author"]))] {
        if !x.is_empty() {
            line.push(x);
        }
    }
    Hit { id: text(&v["id"]), title: text(&v["title"]), snippet: line.join(" · "), url: text(&v["url"]) }
}

/// A story of Algolia's search, in the same words.
fn algolia_hit(v: &Value) -> Hit {
    let url = text(&v["url"]);
    let site = url.split("://").nth(1).and_then(|r| r.split('/').next()).map(|h| h.trim_start_matches("www.").to_string()).unwrap_or_default();
    let mut line = Vec::new();
    for x in [site, format!("{} points", v["points"].as_i64().unwrap_or(0)), format!("{} comments", v["num_comments"].as_i64().unwrap_or(0)), year(&v["created_at"]), format!("by {}", text(&v["author"]))] {
        if !x.is_empty() {
            line.push(x);
        }
    }
    Hit { id: text(&v["objectID"]), title: text(&v["title"]), snippet: line.join(" · "), url }
}

/// A paper of arXiv on ChloeDB's public search.
fn paper_hit(v: &Value) -> Hit {
    let authors = text(&v["authors"]);
    let mut names: Vec<&str> = authors.split(',').map(str::trim).filter(|s| !s.is_empty()).take(3).collect();
    if authors.split(',').count() > 3 {
        names.push("et al.");
    }
    let mut line = vec![names.join(", ")];
    for x in [v["year"].as_i64().map(|y| y.to_string()).unwrap_or_default(), text(&v["pcat"]), format!("{} citations", v["cites"].as_i64().unwrap_or(0))] {
        if !x.is_empty() {
            line.push(x);
        }
    }
    let abs = clip(&text(&v["abstract"]), 300);
    let id = text(&v["id"]);
    Hit { id: id.clone(), title: text(&v["title"]), snippet: format!("{}{}", line.join(" · "), if abs.is_empty() { String::new() } else { format!(" — {abs}") }), url: format!("https://arxiv.org/abs/{id}") }
}

/// Text without its HTML tags and entities.
pub fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = false;
    for c in s.replace("<p>", "\n").chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"").replace("&#x27;", "'").replace("&#x2F;", "/").replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&").trim().to_string()
}

pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_named_with_their_kind() {
        let s = Source::parse("hn=http://hn-app:7850").unwrap();
        assert_eq!((s.name.as_str(), s.kind.clone(), s.base.as_str()), ("hn", Kind::App, "http://hn-app:7850"));
        let s = Source::parse("algolia=algolia-hn:https://hn.algolia.com/api/v1/").unwrap();
        assert_eq!((s.kind.clone(), s.base.as_str()), (Kind::AlgoliaHn, "https://hn.algolia.com/api/v1"));
        for bad in ["hn", "=http://x", "hn=ftp://x", "hn=nope:http://x", "h n=http://x"] {
            assert!(Source::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(token_var("hn-public"), "CHLOEDB_SOURCE_HN_PUBLIC_TOKEN");
    }

    #[test]
    fn rows_of_each_engine_read_alike() {
        let h = algolia_hit(&json!({"objectID": "101", "title": "Announcing Rust 1.0", "url": "https://www.blog.rust-lang.org/x", "points": 1400, "num_comments": 610, "created_at": "2015-05-15T12:00:00Z", "author": "steveklabnik"}));
        assert_eq!(h.snippet, "blog.rust-lang.org · 1400 points · 610 comments · 2015 · by steveklabnik");
        let h2 = hn_hit(&json!({"id": 101, "title": "Announcing Rust 1.0", "url": "https://blog.rust-lang.org/x", "site": "blog.rust-lang.org", "points": 1400, "comments": 610, "created": "2015-05-15T12:00:00Z", "author": "steveklabnik"}));
        assert_eq!((h2.id.as_str(), h2.snippet.as_str()), ("101", h.snippet.as_str()));
        assert_eq!(plain("<p>a &amp; b</p><i>c</i>"), "a & bc");
    }
}
