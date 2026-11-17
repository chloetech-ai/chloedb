//! Feeds: a reader's own weekly digest of arXiv, in the manner of a
//! research newsletter (an issue is "Vol. N" for a span of days: the ten
//! papers that matter most to them, each a paragraph that says who wrote
//! it, what it does and what it found, with its links, then extras: tools
//! and benchmarks). What goes in is theirs to say: topics in their words,
//! papers like some they name, categories, techniques, domains, people,
//! what to leave out, how many, how long, for whom.
//!
//! Choosing the papers is SQL: for each interest, the span's papers by
//! words and meaning fused (`rrf`), papers near the seeds by meaning, then
//! fused across interests here, with a prior for code, citations and the
//! reader's weights, at most a few per topic. Writing the paragraphs is
//! `llm()` on the server, one paper at a time, kept in `ax_notes`. Who a
//! paper is from comes from its page on arxiv.org (the HTML of the paper,
//! its affiliations), read once a paper, gently.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::search::{self, Ctx, COLS_ABS};
use crate::strata::{Row, RowExt};
use crate::fulltext::Full;
use crate::understand;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    /// How much the words and meaning count against the rest.
    pub relevance: f64,
    /// A paper with code.
    pub code: f64,
    /// Citations already (few, for a week-old paper).
    pub citations: f64,
    /// A paper by a person the feed names, or one with many coauthors'
    /// papers behind them (an established group).
    pub authors: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Weights { relevance: 1.0, code: 0.25, citations: 0.15, authors: 0.2 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub name: String,
    pub tagline: String,
    /// Topics in the reader's words ("sequential recommendation").
    pub interests: Vec<String>,
    /// Papers like these (arXiv ids).
    pub seeds: Vec<String>,
    pub categories: Vec<String>,
    pub techniques: Vec<String>,
    pub domains: Vec<String>,
    /// People's keys: their new papers always go in.
    pub authors: Vec<String>,
    /// Words that keep a paper out.
    pub exclude: Vec<String>,
    /// Only these kinds (survey, benchmark, dataset, tool, theory).
    pub kinds: Vec<String>,
    pub code_only: bool,
    /// The span of an issue, in days.
    pub days: i64,
    pub papers: usize,
    /// At most this many papers of one topic.
    pub per_topic: usize,
    /// `recsys` (a paragraph a paper), `brief` (two or three sentences),
    /// `bullets` (the problem, the method, the result).
    pub style: String,
    pub sentences: usize,
    /// For whom: practitioners, researchers, executives, students.
    pub audience: String,
    /// Extras after the papers: tools, benchmarks, surveys.
    pub extras: Vec<String>,
    pub weights: Weights,
    /// Read each chosen paper's page on arxiv.org for its affiliations.
    pub affiliations: bool,
    /// With `full`: the figures shown a paper (0 to 3), its own images
    /// copied from its page.
    pub figures: usize,
    /// Write an issue each week on its own (Mondays, from 06:00 UTC), for
    /// the feed's RSS.
    pub weekly: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            name: "My arXiv digest".into(),
            tagline: String::new(),
            interests: Vec::new(),
            seeds: Vec::new(),
            categories: Vec::new(),
            techniques: Vec::new(),
            domains: Vec::new(),
            authors: Vec::new(),
            exclude: Vec::new(),
            kinds: Vec::new(),
            code_only: false,
            days: 7,
            papers: 10,
            per_topic: 2,
            style: "full".into(),
            sentences: 7,
            audience: "practitioners and researchers".into(),
            extras: vec!["tools".into(), "benchmarks".into()],
            weights: Weights::default(),
            affiliations: true,
            figures: 2,
            weekly: false,
        }
    }
}

/// A ready-made feed: one like the recommender-systems newsletter it is
/// modelled on.
pub fn example() -> Config {
    Config {
        name: "Recommender systems weekly".into(),
        tagline: "The week's most useful papers on recommender systems, search and ranking".into(),
        interests: vec!["recommender systems".into(), "sequential recommendation".into(), "LLM-based recommendation".into(), "click-through rate prediction".into(), "learning to rank".into(), "generative retrieval and semantic IDs".into()],
        categories: vec!["cs.IR".into()],
        ..Config::default()
    }
}

fn filters(cfg: &Config, params: &mut Map<String, Value>, since: &str, until: &str) -> Vec<String> {
    let mut w = vec![format!("created >= TIMESTAMP '{since}'"), format!("created < TIMESTAMP '{until}'")];
    if !cfg.categories.is_empty() || !cfg.techniques.is_empty() || !cfg.domains.is_empty() {
        // Any of them: a category or a technique or a domain it names.
        let mut any = Vec::new();
        for (i, c) in cfg.categories.iter().enumerate() {
            params.insert(format!("cat{i}"), json!(c));
            any.push(format!("cats = :cat{i}"));
        }
        for (i, t) in cfg.techniques.iter().enumerate() {
            params.insert(format!("tech{i}"), json!(t));
            any.push(format!("techniques = :tech{i}"));
        }
        for (i, d) in cfg.domains.iter().enumerate() {
            params.insert(format!("dom{i}"), json!(d));
            any.push(format!("domains = :dom{i}"));
        }
        if cfg.interests.is_empty() && cfg.seeds.is_empty() {
            w.push(format!("({})", any.join(" OR ")));
        } else {
            // With interests, the filters widen the pool rather than cut it:
            // they are scored as one more interest (see `choose`).
            params.insert("_any".into(), json!(any.join(" OR ")));
        }
    }
    if !cfg.kinds.is_empty() {
        let ks: Vec<String> = cfg.kinds.iter().enumerate().map(|(i, k)| {
            params.insert(format!("kind{i}"), json!(k));
            format!("kinds = :kind{i}")
        }).collect();
        w.push(format!("({})", ks.join(" OR ")));
    }
    if cfg.code_only {
        w.push("code != ''".into());
    }
    for (i, x) in cfg.exclude.iter().enumerate() {
        params.insert(format!("x{i}"), json!(x));
        w.push(format!("NOT MATCH(title, :x{i})"));
        w.push(format!("NOT MATCH(abstract, :x{i})"));
    }
    w
}

/// The span an issue covers: the `days` ending at `until` (the newest
/// paper's day when not given).
pub async fn span(c: &Ctx<'_>, days: i64, until: Option<&str>) -> (String, String) {
    let end = match until {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => c.soft("the newest paper", "SELECT MAX(created) AS t FROM ax_papers", Value::Null).await.first().map(|r| r.s("t")).unwrap_or_default(),
    };
    let end_t = chrono::DateTime::parse_from_rfc3339(&end).map(|d| d.timestamp()).or_else(|_| chrono::NaiveDate::parse_from_str(end.get(..10).unwrap_or(""), "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc().timestamp())).unwrap_or_else(|_| chrono::Utc::now().timestamp());
    // The day after the newest, so its papers are in.
    let end_day = (end_t / 86_400 + 1) * 86_400;
    let start = end_day - days.max(1) * 86_400;
    (search::iso(start), search::iso(end_day))
}

/// The papers an issue would hold, best first, with why each is there.
pub async fn choose(c: &Ctx<'_>, cfg: &Config, since: &str, until: &str) -> Vec<Row> {
    let mut params = Map::new();
    let conds = filters(cfg, &mut params, since, until);
    let any = params.remove("_any").and_then(|v| v.as_str().map(str::to_string));
    let wh = search::where_of(&conds, "");
    // The lists to fuse: (name, what the trace calls it, SQL, parameters).
    let mut jobs: Vec<(String, String, String, Map<String, Value>)> = Vec::new();
    for (i, t) in cfg.interests.iter().filter(|t| !t.trim().is_empty()).enumerate().take(12) {
        let mut pr = params.clone();
        pr.insert("q".into(), json!(t));
        pr.insert("v".into(), json!({"embed": t, "field": "emb", "table": "ax_papers"}));
        let sql = format!("SELECT id, score() AS score FROM ax_papers {wh} ORDER BY rrf(bm25(title, :q, synonyms => 1), bm25(abstract, :q, synonyms => 1, min => '60%'), similarity(emb, :v)) DESC LIMIT 60");
        jobs.push((format!("interest:{t}"), format!("interest {}: “{t}” by words and meaning", i + 1), sql, pr));
    }
    for s in cfg.seeds.iter().take(10) {
        let s = understand::clean_id(s);
        let mut pr = params.clone();
        pr.insert("v".into(), json!({"vector_of": s, "table": "ax_papers", "field": "emb"}));
        let sql = format!("SELECT id, score() AS score FROM ax_papers {wh} ORDER BY similarity(emb, :v) DESC LIMIT 40");
        jobs.push((format!("like:{s}"), format!("papers like {s}"), sql, pr));
    }
    if let Some(any) = any.clone() {
        let sql = format!("SELECT id FROM ax_papers {} ORDER BY pop DESC, created DESC LIMIT 60", search::where_of(&conds, &format!("({any})")));
        jobs.push(("filters".into(), "the categories, techniques and domains named".into(), sql, params.clone()));
    }
    if !cfg.authors.is_empty() {
        let mut pr = params.clone();
        pr.insert("au".into(), json!(cfg.authors));
        let sql = format!("SELECT id FROM ax_papers {} ORDER BY created DESC LIMIT 30", search::where_of(&conds, "people IN (:au)"));
        jobs.push(("people".into(), "by the people named".into(), sql, pr));
    }
    if jobs.is_empty() {
        let sql = format!("SELECT id, score() AS score FROM ax_papers {wh} ORDER BY pop DESC, created DESC LIMIT 60");
        jobs.push(("all".into(), "the span's papers".into(), sql, params.clone()));
    }
    let futs: Vec<_> = jobs.iter().map(|(name, what, sql, pr)| {
        let f = c.soft(what, sql, Value::Object(pr.clone()));
        let name = name.clone();
        async move { (name, f.await) }
    }).collect();
    let lists: Vec<(String, Vec<Row>)> = futures_util::future::join_all(futs).await;
    // Fused by rank: each list adds 1 / (60 + rank); a paper several
    // interests find is found more surely.
    let mut fused: HashMap<String, (f64, Vec<String>)> = HashMap::new();
    for (name, rows) in &lists {
        for (rank, r) in rows.iter().enumerate() {
            let e = fused.entry(r.s("id")).or_insert((0.0, Vec::new()));
            let w = if name == "people" { 3.0 } else { 1.0 };
            e.0 += w / (60.0 + rank as f64);
            if !e.1.contains(name) {
                e.1.push(name.clone());
            }
        }
    }
    let mut ids: Vec<(String, f64)> = fused.iter().map(|(k, v)| (k.clone(), v.0)).collect();
    ids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ids.truncate(150);
    if ids.is_empty() {
        return Vec::new();
    }
    let keys: Vec<String> = ids.iter().map(|x| x.0.clone()).collect();
    let rows = c.soft("the candidates", &format!("SELECT {COLS_ABS} FROM ax_papers WHERE id IN (:ids)"), json!({"ids": keys})).await;
    // The authors' standing: the most papers any of them has.
    let mut people: Vec<String> = rows.iter().flat_map(|r| r.list("people").into_iter().take(12)).collect();
    people.sort();
    people.dedup();
    let standing: HashMap<String, i64> = if people.is_empty() {
        HashMap::new()
    } else {
        c.soft("the authors' papers so far", "SELECT id, papers, h FROM ax_authors WHERE id IN (:p)", json!({"p": people})).await.into_iter().map(|r| (r.s("id"), r.i("h"))).collect()
    };
    let rel: HashMap<String, f64> = ids.iter().cloned().collect();
    let maxrel = ids.first().map(|x| x.1).unwrap_or(1.0).max(1e-9);
    let w = &cfg.weights;
    let mut scored: Vec<(f64, Row)> = rows.into_iter().map(|mut r| {
        let id = r.s("id");
        let relv = rel.get(&id).copied().unwrap_or(0.0) / maxrel;
        let code = if r.s("code").is_empty() && r.s("hf").is_empty() { 0.0 } else { 1.0 };
        let cites = (1.0 + r.f("cites")).ln() / 5.0;
        let h = r.list("people").iter().take(12).filter_map(|p| standing.get(p)).copied().max().unwrap_or(0) as f64;
        let group = (1.0 + h).ln() / 4.0;
        let s = w.relevance * relv + w.code * code + w.citations * cites.min(1.5) + w.authors * group.min(1.5);
        r.insert("feed_score".into(), json!((s * 1000.0).round() / 1000.0));
        r.insert("found_by".into(), json!(fused.get(&id).map(|f| f.1.clone()).unwrap_or_default()));
        r.insert("max_h".into(), json!(h));
        (s, r)
    }).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|x| x.1).collect()
}

/// The chosen papers and the extras from the candidates, at most
/// `per_topic` of a topic.
pub fn pick(cfg: &Config, ranked: &[Row]) -> (Vec<Row>, Vec<Row>, Vec<Row>, Vec<Row>) {
    let mut per: HashMap<i64, usize> = HashMap::new();
    let mut main = Vec::new();
    let mut rest = Vec::new();
    for r in ranked {
        let t = if r.get("topic").is_some_and(|v| !v.is_null()) { r.i("topic") } else { -1 - main.len() as i64 };
        let n = per.entry(t).or_insert(0);
        if main.len() < cfg.papers.clamp(1, 30) && *n < cfg.per_topic.max(1) {
            *n += 1;
            main.push(r.clone());
        } else {
            rest.push(r.clone());
        }
    }
    let has = |r: &Row, k: &str| r.list("kinds").iter().any(|x| x == k);
    let want = |e: &str| cfg.extras.iter().any(|x| x == e);
    let tools: Vec<Row> = if want("tools") { rest.iter().filter(|r| has(r, "tool") || (!r.s("code").is_empty() && has(r, "dataset"))).take(5).cloned().collect() } else { Vec::new() };
    let benches: Vec<Row> = if want("benchmarks") { rest.iter().filter(|r| has(r, "benchmark") || has(r, "dataset")).filter(|r| !tools.iter().any(|t| t.s("id") == r.s("id"))).take(5).cloned().collect() } else { Vec::new() };
    let surveys: Vec<Row> = if want("surveys") { rest.iter().filter(|r| has(r, "survey")).take(4).cloned().collect() } else { Vec::new() };
    (main, tools, benches, surveys)
}

/// Who a paper is from: the affiliations on its page on arxiv.org (the
/// paper's HTML), kept in `ax_notes`. One page at a time, three seconds
/// apart, as arXiv asks of programs.
pub struct Pages {
    last: Mutex<Instant>,
    http: reqwest::Client,
}

impl Default for Pages {
    fn default() -> Self {
        Pages { last: Mutex::new(Instant::now() - Duration::from_secs(10)), http: reqwest::Client::builder().user_agent("chloedb-arxiv/0.3 (a research digest; reads one page every 15 s, as robots.txt asks)").timeout(Duration::from_secs(20)).build().unwrap_or_default() }
    }
}

impl Pages {
    async fn wait(&self) {
        loop {
            let wait = {
                let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
                let since = last.elapsed();
                if since >= Duration::from_secs(15) {
                    *last = Instant::now();
                    None
                } else {
                    Some(Duration::from_secs(15) - since)
                }
            };
            match wait {
                None => return,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }

    /// The paper's organisations and the first paragraphs of its text.
    pub async fn read(&self, c: &Ctx<'_>, id: &str) -> (Vec<String>, String) {
        let r = self.paper(c, id).await;
        (r.orgs, r.intro)
    }

    /// The paper's page, read once and kept (`ax_notes`, `full:<id>`): who
    /// it is from, its introduction, and its sections, figures and tables.
    pub async fn paper(&self, c: &Ctx<'_>, id: &str) -> Read {
        let k = format!("full:{id}");
        if let Some(r) = c.gsoft("its page, read before", "SELECT org, text FROM ax_notes WHERE id = :k", json!({"k": k})).await.first() {
            let orgs: Vec<String> = r.s("org").split(" | ").filter(|s| !s.is_empty() && !s.ends_with(':')).map(str::to_string).collect();
            let v: Value = serde_json::from_str(&r.s("text")).unwrap_or(Value::Null);
            let full: Option<Full> = v.get("full").cloned().and_then(|x| serde_json::from_value(x).ok());
            return Read { orgs, intro: v["intro"].as_str().unwrap_or("").to_string(), full };
        }
        self.wait().await;
        let t0 = Instant::now();
        let url = format!("https://arxiv.org/html/{id}");
        let got = match self.http.get(&url).send().await {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            _ => String::new(),
        };
        let (orgs, intro) = parse_page(&got);
        let full = Some(crate::fulltext::parse(&got)).filter(|f| !f.sections.is_empty());
        c.trace.push("its page on arxiv.org (who it is from, its text, figures and tables)", &format!("GET {url}"), &Value::Null, usize::from(!got.is_empty()), t0.elapsed().as_secs_f64() * 1000.0, if got.is_empty() { Some("no HTML version") } else { None });
        let kept = json!({"intro": intro, "full": full});
        let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": "page", "org": orgs.join(" | "), "text": kept.to_string(), "at": chrono::Utc::now().to_rfc3339()})]).await;
        Read { orgs, intro, full }
    }

    /// One of a figure's images, copied from arxiv.org once and kept
    /// (`ax_figures`): its row's id and size, or nothing when arXiv does not
    /// give it.
    pub async fn image(&self, c: &Ctx<'_>, paper: &str, fig: &str, k: usize, img: &crate::fulltext::Image) -> Option<Value> {
        let id = format!("{paper}/{fig}/{k}");
        if let Some(r) = c.gsoft("a figure, copied before", "SELECT id, w, h, mime FROM ax_figures WHERE id = :k", json!({"k": id})).await.first() {
            return Some(json!({"id": r.s("id"), "w": r.i("w"), "h": r.i("h"), "mime": r.s("mime")}));
        }
        if !img.src.starts_with("https://arxiv.org/html/") {
            return None;
        }
        self.wait().await;
        let t0 = Instant::now();
        let r = self.http.get(&img.src).send().await.ok().filter(|r| r.status().is_success())?;
        let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let mime = crate::fulltext::mime(&ct, &img.src)?;
        let bytes = r.bytes().await.ok()?;
        c.trace.push("a figure's image on arxiv.org", &format!("GET {}", img.src), &Value::Null, 1, t0.elapsed().as_secs_f64() * 1000.0, None);
        if bytes.is_empty() || bytes.len() > 3 << 20 {
            return None;
        }
        let row = json!({"id": id, "paper": paper, "fig": fig, "k": k, "mime": mime, "data": crate::fulltext::base64(&bytes), "bytes": bytes.len(), "w": img.w, "h": img.h, "src": img.src, "at": chrono::Utc::now().to_rfc3339()});
        c.g.upsert("ax_figures", vec![row]).await.ok()?;
        Some(json!({"id": id, "w": img.w, "h": img.h, "mime": mime}))
    }
}

/// What a paper's page gave.
#[derive(Debug, Default, Clone)]
pub struct Read {
    pub orgs: Vec<String>,
    pub intro: String,
    pub full: Option<Full>,
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for ch in s.chars() {
        match ch {
            '<' => inside = true,
            '>' => {
                inside = false;
                out.push(' ');
            }
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&nbsp;", " ").replace("&#160;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a span holds, up to the `</span>` that closes it (spans inside it
/// are passed over).
fn span_inner(html: &str) -> &str {
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(k) = html[i..].find('<') {
        let at = i + k;
        let rest = &html[at..];
        if rest.starts_with("</span") {
            if depth == 0 {
                return &html[..at];
            }
            depth -= 1;
        } else if rest.starts_with("<span") {
            depth += 1;
        }
        i = at + 1;
    }
    &html[..html.len().min(400)]
}

/// Affiliations and the introduction from a paper's LaTeXML page.
pub fn parse_page(html: &str) -> (Vec<String>, String) {
    if html.is_empty() {
        return (Vec::new(), String::new());
    }
    let mut orgs: Vec<String> = Vec::new();
    let re = regex::Regex::new(r#"class="ltx_contact ltx_role_affiliation"[^>]*>"#).expect("regex");
    for m in re.find_iter(html) {
        let t = strip_tags(span_inner(&html[m.end()..]));
        let t = t.trim_matches(|c: char| c == ',' || c == ' ' || c.is_ascii_digit() || c == '∗' || c == '*');
        // A label ("Affiliation:") on its own, or in front of the name.
        let t = t.strip_prefix("Affiliations:").or_else(|| t.strip_prefix("Affiliation:")).unwrap_or(t).trim().to_string();
        if t.len() > 2 && t.len() < 160 && !t.ends_with(':') && !orgs.contains(&t) && !t.contains('@') {
            orgs.push(t);
        }
    }
    // Older pages put affiliations in the author block's notes.
    if orgs.is_empty() {
        let re = regex::Regex::new(r#"(?s)<span class="ltx_text ltx_font_italic"[^>]*>([^<]{4,120})</span>"#).expect("regex");
        for m in re.captures_iter(html).take(6) {
            let t = strip_tags(&m[1]);
            if ["University", "Institute", "Lab", "Inc", "Google", "Meta", "Microsoft", "Research", "College", "School", "Center", "Centre", "Academy"].iter().any(|w| t.contains(w)) && !orgs.contains(&t) {
                orgs.push(t);
            }
        }
    }
    let mut intro = String::new();
    if let Some(i) = html.find("id=\"S1\"") {
        let re = regex::Regex::new(r#"(?s)<p class="ltx_p"[^>]*>(.*?)</p>"#).expect("regex");
        for m in re.captures_iter(&html[i..]).take(4) {
            intro.push_str(&strip_tags(&m[1]));
            intro.push(' ');
            if intro.len() > 1800 {
                break;
            }
        }
    }
    let intro: String = intro.chars().take(2000).collect();
    (orgs.into_iter().take(6).collect(), intro.trim().to_string())
}

/// The short name of an organisation: "Google DeepMind, London, UK" is
/// "Google DeepMind".
pub fn short_org(o: &str) -> String {
    let first = o.split(',').next().unwrap_or(o).trim();
    first.trim_start_matches("Department of Computer Science").trim().to_string()
}

fn instructions(cfg: &Config) -> String {
    let n = cfg.sentences.clamp(2, 12);
    match cfg.style.as_str() {
        "bullets" => format!("You write one entry of a weekly arXiv digest for {}. Using only the material given, answer with three short bullets, each starting with a bold label: **Problem**, **Method**, **Result** (with its numbers). No preamble.", cfg.audience),
        "brief" => format!("You write one entry of a weekly arXiv digest for {}. Using only the material given, write 2 or 3 sentences: what the paper does, its main result with a number if there is one, and why it matters. Start with \"This paper from <who>\" naming the organisation(s) or authors given. No hype, no preamble.", cfg.audience),
        _ => format!("You write one entry of a weekly research digest of arXiv papers for {}, in the manner of a respected newsletter. Using only the material given, write one paragraph of about {n} sentences. \
Start with \"This paper from <who>\" naming the organisation(s) given (or \"<first author> et al.\" if none are given), then: the problem and why it matters, what is new in the approach (concretely: architecture, objective, data, training), the key results with their numbers and baselines, and what a reader can take away or reuse. \
Plain, exact, confident language; no hype words (\"groundbreaking\", \"revolutionary\"), no questions, no bullet points, no preamble, no closing line about the future. Never invent numbers, names or claims not in the material.", cfg.audience),
    }
}

/// The paragraph for one paper, kept by its id and the style.
pub async fn blurb(c: &Ctx<'_>, cfg: &Config, model: &str, r: &Row, orgs: &[String], intro: &str) -> String {
    let id = r.s("id");
    let k = format!("blurb:{}:{}:{}:{}", cfg.style, cfg.sentences, cfg.audience.len(), id);
    if let Some(x) = c.gsoft("a kept paragraph", "SELECT text FROM ax_notes WHERE id = :k", json!({"k": k})).await.first() {
        return x.s("text");
    }
    let who = if orgs.is_empty() { crate::answer::first_author(r) } else { orgs.iter().map(|o| short_org(o)).take(3).collect::<Vec<_>>().join(", ") };
    let mut ctx = format!("Title: {}\nAuthors: {}\nFrom: {who}\nAbstract: {}\n", r.s("title"), r.s("authors"), r.s("abstract"));
    if !intro.is_empty() {
        ctx.push_str(&format!("Introduction (excerpt): {intro}\n"));
    }
    let t0 = Instant::now();
    let text = crate::ask::llm(c, &format!("the paragraph for {id} (llm() on the server)"), model, &instructions(cfg), "Write the entry.", &ctx, 2000).await.unwrap_or_default();
    let text = text.trim().to_string();
    if !text.is_empty() {
        let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": cfg.style, "text": text, "org": who, "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": t0.elapsed().as_secs_f64() * 1000.0})]).await;
    }
    text
}

/// What the model is asked for a paper explained in full (`full`).
fn explain_instructions(cfg: &Config) -> String {
    format!("You explain one arXiv paper in a weekly research digest for {}, so that a reader who will not open the paper understands what it did, how, and what it found. Use only the paper's text given. \
Answer with one JSON object and nothing else: \
{{\"brief\": \"two plain sentences: what the paper does and its main result\", \
\"problem\": \"2 to 4 sentences: the problem and why it matters\", \
\"approach\": \"3 to 6 sentences: what is new in the method, concretely (architecture, objective, data, training, how it is used), so that a reader could explain it to a colleague\", \
\"results\": \"2 to 5 sentences: the key results with their numbers, datasets and baselines\", \
\"takeaway\": \"1 to 3 sentences: what a reader can take away or reuse\", \
\"limits\": \"1 or 2 sentences: the limits the paper states or its setup implies; empty if none\", \
\"terms\": [{{\"term\": \"...\", \"meaning\": \"a plain one-sentence definition\"}}], \
\"figures\": [{{\"n\": 1, \"why\": \"one sentence: what the figure shows and what to notice in it\"}}], \
\"table\": {{\"n\": 1, \"why\": \"one sentence: what the table shows\"}}}} \
Rules: terms are up to 3 terms of the paper a newcomer needs. figures are 1 or 2 picked by their numbers [F1], [F2] ...: first the one that best explains the method (an overview or architecture), then the one that shows the main result; [] when none is listed. table is the table of main results by its number [T1] ..., or null when none is listed. \
Start brief with what was done, not with \"This paper\". Plain, exact language; never invent numbers, names or claims that are not in the text; no hype words; no preamble.", cfg.audience)
}

/// A paper explained from its full text (or its abstract when arXiv has no
/// HTML of it), kept by its id and the audience.
pub async fn explain(c: &Ctx<'_>, cfg: &Config, model: &str, r: &Row, orgs: &[String], full: Option<&Full>) -> Value {
    let id = r.s("id");
    let k = format!("explain:1:{}:{}:{id}", cfg.audience.len(), usize::from(full.is_some()));
    if let Some(x) = c.gsoft("a kept explanation", "SELECT text FROM ax_notes WHERE id = :k", json!({"k": k})).await.first() {
        if let Ok(v) = serde_json::from_str::<Value>(&x.s("text")) {
            return v;
        }
    }
    let who = if orgs.is_empty() { crate::answer::first_author(r) } else { orgs.iter().map(|o| short_org(o)).take(3).collect::<Vec<_>>().join(", ") };
    let mut ctx = format!("Title: {}\nAuthors: {}\nFrom: {who}\nAbstract: {}\n", r.s("title"), r.s("authors"), r.s("abstract"));
    if let Some(f) = full {
        ctx.push_str(&f.context());
    }
    let t0 = Instant::now();
    let text = crate::ask::llm(c, &format!("the explanation of {id} from its full text (llm() on the server)"), model, &explain_instructions(cfg), "Explain the paper.", &ctx, 4000).await.unwrap_or_default();
    let Some(v) = crate::ask::json_in(&text).filter(|v| v.get("brief").and_then(|b| b.as_str()).is_some_and(|b| !b.is_empty())) else {
        return Value::Null;
    };
    let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": "full", "text": v.to_string(), "org": who, "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": t0.elapsed().as_secs_f64() * 1000.0})]).await;
    v
}

/// The opening of an issue: what its papers share and where they differ.
async fn opening(c: &Ctx<'_>, cfg: &Config, model: &str, papers: &[Value]) -> String {
    let mut ctx = String::new();
    for (i, p) in papers.iter().enumerate() {
        ctx.push_str(&format!("[{}] {}: {}\n", i + 1, p["title"].as_str().unwrap_or(""), p["brief"].as_str().or(p["blurb"].as_str()).unwrap_or("")));
    }
    let ins = format!("You write the opening of a weekly research digest for {}: 3 or 4 sentences on what this issue's papers have in common, where they differ, and which to read first and why, naming papers by their number in brackets, like [2]. Use only the summaries given; plain language, no hype, no greeting, no preamble.", cfg.audience);
    crate::ask::llm(c, "the issue's opening (llm() on the server)", model, &ins, "Write the opening.", &ctx, 1200).await.unwrap_or_default().trim().to_string()
}

/// Dates as an issue's title gives them: "Nov 8 – Nov 14, 2026".
pub fn span_text(since: &str, until: &str) -> String {
    let p = |s: &str| chrono::DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&chrono::Utc)).ok();
    match (p(since), p(until)) {
        (Some(a), Some(b)) => {
            let b = b - chrono::Duration::days(1);
            format!("{} – {}", a.format("%b %-d"), b.format("%b %-d, %Y"))
        }
        _ => format!("{since} – {until}"),
    }
}

/// An issue's Markdown, ready to paste into a newsletter.
pub fn markdown(cfg: &Config, issue: &Value) -> String {
    let mut s = format!("# {} — Vol. {} for {}\n\n", cfg.name, issue["vol"], issue["span"].as_str().unwrap_or(""));
    if !cfg.tagline.is_empty() {
        s.push_str(&format!("*{}*\n\n", cfg.tagline));
    }
    if let Some(o) = issue["intro"].as_str().filter(|o| !o.is_empty()) {
        s.push_str(&format!("{o}\n\n"));
    }
    let txt = |v: &Value| v.as_str().unwrap_or("").trim().to_string();
    for (i, p) in issue["papers"].as_array().into_iter().flatten().enumerate() {
        if p.get("brief").is_some_and(|b| b.is_string()) {
            s.push_str(&format!("## {}. {}\n\n", i + 1, txt(&p["title"])));
            let orgs: Vec<String> = p["orgs"].as_array().into_iter().flatten().filter_map(|o| o.as_str()).take(4).map(short_org).collect();
            if !orgs.is_empty() {
                s.push_str(&format!("*{}*\n\n", orgs.join(" · ")));
            }
            s.push_str(&format!("**In brief.** {}\n\n", txt(&p["brief"])));
            let figs = p["figures"].as_array().cloned().unwrap_or_default();
            if let Some(f) = figs.first() {
                for src in f["src"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                    s.push_str(&format!("![{}]({src})\n", txt(&f["label"])));
                }
                s.push_str(&format!("*{}: {}*\n\n", txt(&f["label"]), if txt(&f["why"]).is_empty() { txt(&f["caption"]) } else { txt(&f["why"]) }));
            }
            for (k, title) in [("problem", "The problem"), ("approach", "The approach"), ("results", "The results"), ("takeaway", "Takeaway"), ("limits", "Limits")] {
                if !txt(&p[k]).is_empty() {
                    s.push_str(&format!("**{title}.** {}\n\n", txt(&p[k])));
                }
            }
            for f in figs.iter().skip(1) {
                for src in f["src"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                    s.push_str(&format!("![{}]({src})\n", txt(&f["label"])));
                }
                s.push_str(&format!("*{}: {}*\n\n", txt(&f["label"]), if txt(&f["why"]).is_empty() { txt(&f["caption"]) } else { txt(&f["why"]) }));
            }
            if let Some(rows) = p["table"]["rows"].as_array().filter(|r| r.len() >= 2) {
                s.push_str(&format!("*{}: {}*\n\n", txt(&p["table"]["label"]), if txt(&p["table"]["why"]).is_empty() { txt(&p["table"]["caption"]) } else { txt(&p["table"]["why"]) }));
                let width = rows.iter().filter_map(|r| r.as_array().map(|a| a.len())).max().unwrap_or(0);
                for (j, r) in rows.iter().enumerate() {
                    let mut cells: Vec<String> = r.as_array().into_iter().flatten().map(|c| c.as_str().unwrap_or("").replace('|', "\\|")).collect();
                    cells.resize(width, String::new());
                    s.push_str(&format!("| {} |\n", cells.join(" | ")));
                    if j == 0 {
                        s.push_str(&format!("|{}\n", " --- |".repeat(width)));
                    }
                }
                s.push('\n');
            }
            let terms: Vec<String> = p["terms"].as_array().into_iter().flatten().filter_map(|t| Some(format!("**{}**: {}", t["term"].as_str()?, t["meaning"].as_str()?))).collect();
            if !terms.is_empty() {
                s.push_str(&format!("Terms: {}\n\n", terms.join("; ")));
            }
        } else {
            s.push_str(&format!("## {}. {}\n\n{}\n\n", i + 1, p["title"].as_str().unwrap_or(""), p["blurb"].as_str().unwrap_or("")));
        }
        s.push_str(&format!("📚 https://arxiv.org/abs/{}\n", p["id"].as_str().unwrap_or("")));
        if let Some(c) = p["code"].as_str().filter(|c| !c.is_empty()) {
            s.push_str(&format!("👨🏽‍💻 {c}\n"));
        }
        if let Some(h) = p["hf"].as_str().filter(|c| !c.is_empty()) {
            s.push_str(&format!("🤗 {h}\n"));
        }
        s.push('\n');
    }
    for (key, title) in [("tools", "Extras: Tools"), ("benchmarks", "Extras: Benchmarks"), ("surveys", "Extras: Surveys")] {
        let xs = issue[key].as_array().cloned().unwrap_or_default();
        if xs.is_empty() {
            continue;
        }
        s.push_str(&format!("## {title}\n\n"));
        for x in xs {
            let link = x["code"].as_str().filter(|c| !c.is_empty()).map(str::to_string).unwrap_or_else(|| format!("https://arxiv.org/abs/{}", x["id"].as_str().unwrap_or("")));
            s.push_str(&format!("- **{}** — {} {}\n", x["title"].as_str().unwrap_or(""), x["line"].as_str().unwrap_or(""), link));
        }
        s.push('\n');
    }
    s
}

fn first_sentence(a: &str) -> String {
    let cut = a.find(". ").map(|i| &a[..=i]).unwrap_or(a);
    cut.chars().take(220).collect()
}

/// Writes an issue: chooses the papers, reads who each is from, writes
/// each paragraph, and keeps the issue. `progress` hears each step as an
/// object: `phase` (choose, chosen, read, explain, figure, write, opening,
/// save), `step` (the step in words), and where it applies `i` and `n`
/// (the paper, from 0, of how many), `id`, `title`, `left_s` (seconds
/// left at most, when no page was read before) and, once, `papers` (the
/// chosen papers' ids and titles) and `candidates`.
pub async fn write_issue(c: &Ctx<'_>, pages: &Pages, feed_id: &str, cfg: &Config, model: &str, until: Option<&str>, progress: &(dyn Fn(Value) + Sync)) -> Result<Value, String> {
    let t0 = Instant::now();
    let (since, until) = span(c, cfg.days, until).await;
    progress(json!({"phase": "choose", "step": format!("choosing papers from {}", span_text(&since, &until))}));
    let ranked = choose(c, cfg, &since, &until).await;
    let (main, tools, benches, surveys) = pick(cfg, &ranked);
    if main.is_empty() {
        return Err("no papers in that span match the feed".into());
    }
    let full_style = cfg.style == "full";
    let n = main.len();
    // A guess at each paper's seconds, before any is done: its page and
    // figure images read 15 s apart (when not read before), and the model.
    let per_paper = if full_style { (1 + cfg.figures.min(3) * 2) * 15 + 30 } else { 8 + if cfg.affiliations { 15 } else { 0 } };
    progress(json!({"phase": "chosen", "step": format!("chose {n} papers from {} candidates", ranked.len()), "n": n, "candidates": ranked.len(), "left_s": n * per_paper,
        "papers": main.iter().map(|r| json!({"id": r.s("id"), "title": r.s("title")})).collect::<Vec<_>>()}));
    let mut papers = Vec::new();
    for (i, r) in main.iter().enumerate() {
        let id = r.s("id");
        let left = n - i;
        let at = |phase: &str, step: String| json!({"phase": phase, "step": step, "i": i, "n": n, "id": r.s("id"), "title": r.s("title"), "left_s": left * per_paper});
        if full_style {
            progress(at("read", format!("reading {} of {n}: {} (its page on arxiv.org, where pages and figures are read 15 s apart, as arXiv asks)", i + 1, r.s("title"))));
        }
        let read = if cfg.affiliations || full_style { pages.paper(c, &id).await } else { Read::default() };
        let mut p = json!({"id": id, "title": r.s("title"), "authors": r.s("authors"), "orgs": read.orgs, "code": r.s("code"), "hf": r.s("hf"), "created": r.s("created"), "cats": r.get("cats"), "techniques": r.get("techniques"), "topic": r.get("topic"), "score": r.get("feed_score"), "found_by": r.get("found_by")});
        if full_style {
            progress(at("explain", format!("explaining {} of {n} from its {}: {}", i + 1, if read.full.is_some() { "full text" } else { "abstract (arXiv has no HTML of it)" }, r.s("title"))));
            let ex = explain(c, cfg, model, r, &read.orgs, read.full.as_ref()).await;
            if ex.is_null() {
                // The model did not answer as asked: the paragraph instead.
                p["blurb"] = json!(blurb(c, cfg, model, r, &read.orgs, &read.intro).await);
            } else {
                for k in ["brief", "problem", "approach", "results", "takeaway", "limits", "terms"] {
                    p[k] = ex[k].clone();
                }
                p["blurb"] = ex["brief"].clone();
            }
            p["full_text"] = json!(read.full.is_some());
            if let Some(f) = &read.full {
                // The figures the model picked (else the first), their images copied.
                let mut picks: Vec<(usize, String)> = ex["figures"].as_array().into_iter().flatten().filter_map(|x| Some(((x["n"].as_u64()? as usize).checked_sub(1)?, x["why"].as_str().unwrap_or("").to_string()))).filter(|(n, _)| *n < f.figures.len()).collect();
                picks.dedup_by_key(|x| x.0);
                if picks.is_empty() && !f.figures.is_empty() {
                    picks.push((0, String::new()));
                }
                let mut figs = Vec::new();
                for (n, why) in picks.into_iter().take(cfg.figures.min(3)) {
                    let fig = &f.figures[n];
                    let mut ev = at("figure", format!("copying {} of paper {} of {} from arxiv.org: {}", fig.label, i + 1, main.len(), r.s("title")));
                    ev["figure"] = json!(fig.label);
                    progress(ev);
                    let mut imgs = Vec::new();
                    for (k, img) in fig.images.iter().take(3).enumerate() {
                        if let Some(x) = pages.image(c, &id, &fig.id, k, img).await {
                            imgs.push(x);
                        }
                    }
                    if !imgs.is_empty() {
                        figs.push(json!({"label": fig.label, "caption": crate::fulltext::text_of(&fig.caption).chars().take(600).collect::<String>(), "why": why, "images": imgs, "src": fig.images.iter().take(3).map(|x| x.src.clone()).collect::<Vec<_>>(), "anchor": format!("https://arxiv.org/html/{id}#{}", fig.id)}));
                    }
                }
                p["figures"] = json!(figs);
                let t = ex["table"]["n"].as_u64().and_then(|n| (n as usize).checked_sub(1)).filter(|n| *n < f.tables.len());
                if let Some(t) = t {
                    let tb = &f.tables[t];
                    p["table"] = json!({"label": tb.label, "caption": tb.caption, "why": ex["table"]["why"], "rows": tb.rows.iter().take(14).collect::<Vec<_>>(), "anchor": format!("https://arxiv.org/html/{id}#{}", tb.id)});
                }
            }
        } else {
            progress(at("write", format!("writing {} of {n}: {}", i + 1, r.s("title"))));
            p["blurb"] = json!(blurb(c, cfg, model, r, &read.orgs, &read.intro).await);
        }
        papers.push(p);
    }
    let intro = if full_style && papers.len() > 1 {
        progress(json!({"phase": "opening", "step": "writing the issue's opening", "i": n, "n": n, "left_s": 0}));
        opening(c, cfg, model, &papers).await
    } else {
        String::new()
    };
    let line = |rows: &[Row]| -> Vec<Value> { rows.iter().map(|r| json!({"id": r.s("id"), "title": r.s("title"), "line": first_sentence(&r.s("abstract")), "code": r.s("code")})).collect() };
    let prior = c.gsoft("the feed's issues so far", "SELECT COUNT(*) AS n FROM ax_issues WHERE feed = :f", json!({"f": feed_id})).await.first().map(|r| r.i("n")).unwrap_or(0);
    let vol = prior + 1;
    let mut issue = json!({
        "feed": feed_id, "vol": vol, "since": since, "until": until, "span": span_text(&since, &until),
        "intro": intro, "style": cfg.style,
        "papers": papers, "tools": line(&tools), "benchmarks": line(&benches), "surveys": line(&surveys),
        "candidates": ranked.len(), "model": model,
    });
    progress(json!({"phase": "save", "step": format!("keeping Vol. {vol}"), "i": n, "n": n, "left_s": 0}));
    let md = markdown(cfg, &issue);
    issue["markdown"] = json!(md);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    issue["ms"] = json!(ms);
    let id = format!("{feed_id}:{vol}");
    let ids: Vec<String> = main.iter().map(|r| r.s("id")).collect();
    c.g.upsert("ax_issues", vec![json!({"id": id, "feed": feed_id, "vol": vol, "since": since, "until": until, "made": chrono::Utc::now().to_rfc3339(), "title": format!("{} — Vol. {vol} for {}", cfg.name, span_text(&since, &until)), "body": issue.to_string(), "papers": ids, "ms": ms})]).await.map_err(|e| e.to_string())?;
    issue["id"] = json!(id);
    Ok(issue)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A feed's issues as RSS 2.0, newest first, each issue's papers as HTML.
/// Feeds of one reader that write their issues on their own, at most.
pub const WEEKLY_PER_READER: usize = 3;
/// Issues written on their own in one hourly round, at most: each reads
/// its papers' pages on arxiv.org 15 seconds apart.
pub const WEEKLY_PER_ROUND: usize = 6;

/// The start of this week's issues: the last Monday at 06:00 UTC at or before `now`.
pub fn week_start(now: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    use chrono::{Datelike, TimeZone};
    let back = now.weekday().num_days_from_monday() as i64;
    let monday = (now - chrono::Duration::days(back)).date_naive();
    let start = chrono::Utc.from_utc_datetime(&monday.and_hms_opt(6, 0, 0).unwrap_or_default());
    if start > now { start - chrono::Duration::days(7) } else { start }
}

/// The weekly feeds due an issue: none made since this week's start,
/// the longest waiting first.
pub fn due(feeds: &[(String, Config)], last: &std::collections::HashMap<String, String>, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    let start = week_start(now).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut d: Vec<(String, String)> = feeds
        .iter()
        .filter(|(_, c)| c.weekly)
        .map(|(id, _)| (last.get(id).cloned().unwrap_or_default(), id.clone()))
        .filter(|(m, _)| m.as_str() < start.as_str())
        .collect();
    d.sort();
    d.into_iter().map(|(_, id)| id).take(WEEKLY_PER_ROUND).collect()
}

pub fn rss(feed_id: &str, cfg: &Config, issues: &[Row], base: &str) -> String {
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel><title>{}</title><link>{base}/#/feed/{feed_id}</link><description>{}</description>\n", esc(&cfg.name), esc(if cfg.tagline.is_empty() { "A digest of arXiv, made by ChloeDB" } else { &cfg.tagline }));
    for r in issues {
        let issue: Value = serde_json::from_str(&r.s("body")).unwrap_or(Value::Null);
        let mut html = String::new();
        for (i, p) in issue["papers"].as_array().into_iter().flatten().enumerate() {
            let id = p["id"].as_str().unwrap_or("");
            html.push_str(&format!("<h3>{}. <a href=\"https://arxiv.org/abs/{id}\">{}</a></h3><p>{}</p>", i + 1, esc(p["title"].as_str().unwrap_or("")), esc(p["blurb"].as_str().unwrap_or(""))));
            if let Some(cu) = p["code"].as_str().filter(|x| !x.is_empty()) {
                html.push_str(&format!("<p>Code: <a href=\"{0}\">{0}</a></p>", esc(cu)));
            }
        }
        let date = chrono::DateTime::parse_from_rfc3339(&r.s("made")).map(|d| d.to_rfc2822()).unwrap_or_default();
        s.push_str(&format!("<item><title>{}</title><guid isPermaLink=\"false\">{}</guid><link>{base}/#/issue/{}</link><pubDate>{date}</pubDate><description>{}</description></item>\n", esc(&r.s("title")), esc(&r.s("id")), esc(&r.s("id")), esc(&html)));
    }
    s.push_str("</channel></rss>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_issues_are_due_from_monday_morning() {
        use chrono::TimeZone;
        let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc);
        // A Monday before 06:00 belongs to the week before.
        assert_eq!(week_start(at("2026-11-16T05:59:00Z")), chrono::Utc.with_ymd_and_hms(2026, 11, 9, 6, 0, 0).unwrap());
        assert_eq!(week_start(at("2026-11-16T06:00:00Z")), chrono::Utc.with_ymd_and_hms(2026, 11, 16, 6, 0, 0).unwrap());
        assert_eq!(week_start(at("2026-11-19T12:00:00Z")), chrono::Utc.with_ymd_and_hms(2026, 11, 16, 6, 0, 0).unwrap());
        let on = Config { weekly: true, ..Config::default() };
        let feeds = vec![("a".to_string(), on.clone()), ("b".to_string(), on.clone()), ("c".to_string(), Config::default()), ("d".to_string(), on)];
        let last: std::collections::HashMap<String, String> = [("a", "2026-11-16T07:00:00Z"), ("b", "2026-11-10T07:00:00Z")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        // a has this week's; d never had one, so it waits longest; c is not weekly.
        assert_eq!(due(&feeds, &last, at("2026-11-17T00:00:00Z")), vec!["d", "b"]);
    }

    #[test]
    fn pages() {
        let html = r#"<div class="ltx_authors"><span class="ltx_contact ltx_role_affiliation"><sup>1</sup>Google DeepMind, London</span><span class="ltx_contact ltx_role_affiliation">Stanford University</span><span class="ltx_contact ltx_role_affiliation"><span class="ltx_text ltx_font_bold">Affiliation:</span> Kuaishou <span class="ltx_text">Technology</span></span></div><section id="S1"><p class="ltx_p">Recommender systems are everywhere.</p></section>"#;
        let (orgs, intro) = parse_page(html);
        assert_eq!(orgs, vec!["Google DeepMind, London", "Stanford University", "Kuaishou Technology"]);
        assert_eq!(intro, "Recommender systems are everywhere.");
        assert_eq!(short_org("Google DeepMind, London"), "Google DeepMind");
    }

    #[test]
    fn markdown_of_an_explained_issue() {
        let cfg = Config { name: "RecSys weekly".into(), ..Config::default() };
        assert_eq!(cfg.style, "full");
        let issue = json!({"vol": 3, "span": "Nov 8 – Nov 14, 2026", "intro": "Two papers on retrieval [1] [2].", "papers": [
            {"id": "2611.00001", "title": "Semantic IDs at scale", "orgs": ["Google DeepMind, London, UK"], "brief": "They index items by learned codes.", "problem": "Item ids do not generalise.", "approach": "An RQ-VAE assigns codes.", "results": "Recall@10 rises 12%.", "takeaway": "Use codes.", "limits": "",
             "terms": [{"term": "RQ-VAE", "meaning": "a quantiser of vectors"}],
             "figures": [{"label": "Figure 1", "why": "The pipeline.", "caption": "Overview.", "src": ["https://arxiv.org/html/2611.00001v1/x1.png"]}],
             "table": {"label": "Table 2", "why": "Main results.", "rows": [["Model", "R@10"], ["Ours", "0.42"]]}, "code": "https://github.com/x/y"},
            {"id": "2611.00002", "title": "Old style", "blurb": "A paragraph."}
        ]});
        let md = markdown(&cfg, &issue);
        assert!(md.starts_with("# RecSys weekly — Vol. 3 for Nov 8 – Nov 14, 2026\n\nTwo papers on retrieval [1] [2].\n\n## 1. Semantic IDs at scale\n\n*Google DeepMind*\n\n**In brief.** They index items by learned codes.\n\n![Figure 1](https://arxiv.org/html/2611.00001v1/x1.png)\n*Figure 1: The pipeline.*\n\n**The problem.**"), "{md}");
        assert!(md.contains("| Model | R@10 |\n| --- | --- |\n| Ours | 0.42 |"), "{md}");
        assert!(md.contains("Terms: **RQ-VAE**: a quantiser of vectors"), "{md}");
        assert!(!md.contains("**Limits.**"), "{md}");
        assert!(md.contains("## 2. Old style\n\nA paragraph."), "{md}");
    }

    #[test]
    fn spans() {
        assert_eq!(span_text("2026-11-08T00:00:00Z", "2026-11-15T00:00:00Z"), "Nov 8 – Nov 14, 2026");
    }
}
