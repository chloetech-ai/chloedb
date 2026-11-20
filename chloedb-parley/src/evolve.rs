//! How agents learn and whether the work gets richer (Python's `evolve.py`):
//! a post's features and richness read from its words, drawing or page; an
//! agent's memory (ChloeCtx, else rows of `agent_notes`); the Evolution page.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Instant;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Value};

use crate::app::App;
use crate::db::{record, Row, RowExt};
use crate::util::{epoch, iso, new_id, now, round, Error, Result};

pub const KINDS: [&str; 3] = ["knowledge", "style", "craft"];
pub const STYLE: [&str; 14] = [
    "analogy",
    "example",
    "numbers",
    "contrast",
    "because",
    "story",
    "source",
    "conversation",
    "hook",
    "bullets",
    "steps",
    "bold",
    "question",
    "credit",
];
pub const KNOWLEDGE: [&str; 4] = ["numbers", "source", "names", "link"];
pub const COMMONS: &str = "pa_commons";

/// What older rows said, and what they mean now.
pub fn old_kind(k: &str) -> &str {
    match k {
        "drawing" | "picture" => "craft",
        "meme" | "writing" | "any" => "style",
        k => k,
    }
}

fn kind_of(k: &str) -> &'static str {
    match old_kind(k) {
        "knowledge" => "knowledge",
        "craft" => "craft",
        _ => "style",
    }
}

const STYLE_FEATURES: [(&str, &str); 8] = [
    (
        "analogy",
        r"\b(like an?|as if|imagine|think of (it|them) as|the way an?)\b",
    ),
    (
        "example",
        r"\b(for example|for instance|e\.g\.|such as|take the)\b",
    ),
    (
        "numbers",
        r"\b\d[\d,.]*\s?(%|(km|kg|mm|cm|m|g|years?|hours?|days?|million|billion|thousand|x|°C?|ms|GB|MB|kWh|W)\b)|\b\d{1,3}(,\d{3})+\b|\b\d+\.\d+\b|\b(1[5-9]|20)\d\d\b",
    ),
    (
        "contrast",
        r"\b(but|however|whereas|on the other hand|instead|yet)\b",
    ),
    (
        "because",
        r"\b(because|so that|which means|that's why|that is why)\b",
    ),
    (
        "story",
        r"\b(I|we) (once|tried|found|saw|spent|remember|learned|built|watched)\b",
    ),
    (
        "source",
        r"https?://|\baccording to\b|\b(a|the) (paper|study|survey|report) (by|from|in)\b",
    ),
    ("conversation", r"@[a-z0-9_]{3,}"),
];

pub const SVG_FEATURES: [(&str, &str); 16] = [
    ("gradient", r"<(linear|radial)Gradient\b"),
    ("radial", r"<radialGradient\b"),
    ("blur", r"<feGaussianBlur\b"),
    ("texture", r"<feTurbulence\b"),
    ("lighting", r"<fe(Diffuse|Specular)Lighting\b"),
    ("displace", r"<feDisplacementMap\b"),
    ("pattern", r"<pattern\b"),
    ("clip", r"<clipPath\b"),
    ("mask", r"<mask\b"),
    ("reuse", r"<use\b"),
    ("curves", r#"<path\b[^>]*\bd="[^"]*[CcQqSsAa]"#),
    ("transparency", r#"\b(fill-)?opacity="0?\.\d"#),
    ("transform", r#"\btransform="[^"]*(rotate|skew|scale)"#),
    ("lettering", r"<text\b"),
    ("blend", r"mix-blend-mode|<feBlend\b"),
    ("stroke-art", r"stroke-dasharray|stroke-linecap"),
];

const APP_FEATURES: [(&str, &str); 10] = [
    ("canvas", r"<canvas\b|getContext\("),
    (
        "animation",
        r"requestAnimationFrame|@keyframes|setInterval\(",
    ),
    (
        "interactive",
        r"addEventListener\(|onclick=|oninput=|onchange=",
    ),
    ("controls", r"<(input|select|button|textarea)\b"),
    ("svg", r"<svg\b"),
    ("parley-data", r"parley\.get\("),
    ("randomness", r"Math\.random"),
    ("layout", r"display:\s*(grid|flex)"),
    ("sound", r"AudioContext"),
    ("state", r"\bstate\b|let \w+ = \[\]"),
];

struct Rx {
    style: Vec<(&'static str, Regex)>,
    svg: Vec<(&'static str, Regex)>,
    app: Vec<(&'static str, Regex)>,
    colour: Regex,
    element: Regex,
    name: Regex,
    step: Regex,
    link: Regex,
    credit: Regex,
}

fn rx() -> &'static Rx {
    static R: OnceLock<Rx> = OnceLock::new();
    R.get_or_init(|| Rx {
        style: STYLE_FEATURES
            .iter()
            .map(|(n, p)| {
                (
                    *n,
                    Regex::new(&if *n == "conversation" {
                        p.to_string()
                    } else {
                        format!("(?i){p}")
                    })
                    .unwrap(),
                )
            })
            .collect(),
        svg: SVG_FEATURES
            .iter()
            .map(|(n, p)| (*n, Regex::new(p).unwrap()))
            .collect(),
        app: APP_FEATURES
            .iter()
            .map(|(n, p)| (*n, Regex::new(p).unwrap()))
            .collect(),
        colour: Regex::new(r"#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b|rgba?\([^)]*\)|hsla?\([^)]*\)")
            .unwrap(),
        element: Regex::new(r"<(rect|circle|ellipse|line|polyline|polygon|path|text|use|g)\b")
            .unwrap(),
        name: Regex::new(r"^[A-Z][a-z]{2,}(?:\s[A-Z][a-z]+)*").unwrap(),
        step: Regex::new(r"^\s*\d+[.)] ").unwrap(),
        link: Regex::new(r"https?://").unwrap(),
        credit: Regex::new(r"\bafter @[a-z0-9_]+").unwrap(),
    })
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Proper names in a text: capitalised words not at its start nor after the
/// end of a sentence (Python's lookbehinds, walked by hand).
fn proper_names(body: &str) -> HashSet<String> {
    let r = rx();
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let mut out = HashSet::new();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        let ok = i > 0
            && c.is_ascii_uppercase()
            && !word_char(chars[i - 1].1)
            && !(i >= 2
                && matches!(chars[i - 2].1, '.' | '!' | '?')
                && chars[i - 1].1.is_whitespace());
        if ok {
            if let Some(m) = r.name.find(&body[pos..]) {
                let s = m.as_str();
                if !["The", "This", "That", "What", "Which"].contains(&s) {
                    out.insert(s.to_string());
                }
                let end = pos + m.end();
                while i < chars.len() && chars[i].0 < end {
                    i += 1;
                }
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A post's features and its richness (0 to 100).
pub fn craft(kind: &str, body: &str, svg: Option<&str>, app: Option<&str>) -> Value {
    let r = rx();
    let mut feats: Vec<&str> = Vec::new();
    let (mut elements, mut colours) = (0usize, 0usize);
    if let Some(app) = app.filter(|a| !a.is_empty()) {
        for (name, re) in &r.app {
            if re.is_match(app) {
                feats.push(name);
            }
        }
        let lines = app.matches('\n').count() + 1;
        let richness = 7.0 * feats.len() as f64
            + (lines as f64 / 8.0).min(30.0)
            + if body.contains("bullets") || body.contains("**") {
                6.0
            } else {
                0.0
            };
        return json!({"features": feats, "richness": round(richness.min(100.0), 1), "elements": lines, "colours": 0, "style": 0, "knowledge": 0});
    }
    let svg = svg.filter(|s| !s.is_empty());
    if let Some(svg) = svg {
        for (name, re) in &r.svg {
            if re.is_match(svg) {
                feats.push(name);
            }
        }
        elements = r.element.find_iter(svg).count();
        colours = r
            .colour
            .find_iter(svg)
            .map(|m| m.as_str().to_lowercase())
            .collect::<HashSet<_>>()
            .len();
        if elements >= 40 {
            feats.push("dense");
        }
        if colours >= 6 {
            feats.push("palette");
        }
    }
    let lines: Vec<&str> = body.lines().collect();
    for (name, re) in &r.style {
        if re.is_match(body) {
            feats.push(name);
        }
    }
    let first = lines.first().map(|l| l.trim()).unwrap_or("");
    if lines.len() >= 3 && !first.is_empty() && first.chars().count() <= 90 {
        feats.push("hook");
    }
    if proper_names(body).len() >= 2 {
        feats.push("names");
    }
    if lines
        .iter()
        .filter(|l| {
            let l = l.trim_start();
            l.starts_with("- ") || l.starts_with("* ") || l.starts_with("• ")
        })
        .count()
        >= 2
    {
        feats.push("bullets");
    }
    if lines.iter().filter(|l| r.step.is_match(l)).count() >= 2 {
        feats.push("steps");
    }
    if body.matches("**").count() >= 2 {
        feats.push("bold");
    }
    if body.contains('?') {
        feats.push("question");
    }
    if r.link.is_match(body) {
        feats.push("link");
    }
    if r.credit.is_match(&body.to_lowercase()) {
        feats.push("credit");
    }
    let words = body.split_whitespace().count();
    let style = feats.iter().filter(|f| STYLE.contains(f)).count();
    let know = feats.iter().filter(|f| KNOWLEDGE.contains(f)).count();
    let richness = if svg.is_some() {
        4.0 * feats
            .iter()
            .filter(|f| !STYLE.contains(f) && !KNOWLEDGE.contains(f))
            .count() as f64
            + (elements as f64 * 0.6).min(30.0)
            + (colours as f64 * 2.0).min(14.0)
            + 2.0 * style as f64
            + 2.0 * know as f64
    } else {
        5.0 * style as f64
            + 6.0 * know as f64
            + (words as f64 / 4.0).min(30.0)
            + if kind == "picture" || kind == "meme" {
                12.0
            } else {
                0.0
            }
    };
    json!({"features": feats, "richness": round(richness.min(100.0), 1), "elements": elements, "colours": colours, "style": style, "knowledge": know})
}

pub fn space_of(handle: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let s = format!(
        "pa_{}",
        crate::util::re(&R, "[^a-z0-9_]").replace_all(&handle.to_lowercase(), "_")
    );
    s.chars().take(48).collect()
}

impl App {
    pub fn memory_kind(&self) -> &'static str {
        if self.ctx_url.is_some() {
            "chloectx"
        } else {
            "table"
        }
    }

    async fn ctx(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let url = self.ctx_url.clone().unwrap_or_default();
        let t0 = Instant::now();
        let mut req = self
            .db
            .http
            .request(
                method.clone(),
                format!("{}{path}", url.trim_end_matches('/')),
            )
            .timeout(std::time::Duration::from_secs(6));
        if let Some(t) = &self.ctx_token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = &body {
            req = req.json(b);
        }
        let r = req.send().await;
        record(
            &format!("chloectx {method} {}", path.split('?').next().unwrap_or("")),
            &format!("-- ChloeCtx: {method} {path}"),
            &body.clone().filter(|b| b.is_object()).unwrap_or(json!({})),
            t0,
            Some(0),
        );
        let r = r.map_err(|e| Error::new(502, format!("ChloeCtx: {e}")))?;
        let status = r.status();
        let j: Value = r.json().await.unwrap_or(json!({}));
        if !status.is_success() {
            let code = if status.as_u16() >= 500 {
                502
            } else {
                status.as_u16()
            };
            return Err(Error::new(
                code,
                format!(
                    "ChloeCtx: {}",
                    j["error"]
                        .as_str()
                        .unwrap_or(status.canonical_reason().unwrap_or("error"))
                ),
            ));
        }
        Ok(j)
    }

    async fn ctx_space(&self, name: &str, title: &str, about: &str) -> Result<()> {
        if self.ctx_made.lock().unwrap().contains(name) {
            return Ok(());
        }
        self.ctx(
            reqwest::Method::POST,
            "/v1/spaces",
            Some(json!({"name": name, "title": title, "description": about})),
        )
        .await?;
        self.ctx_made.lock().unwrap().insert(name.to_string());
        Ok(())
    }

    /// A note in the agent's memory (and the commons). Never fails: a memory
    /// that is down must not stop a turn.
    pub async fn remember(
        &self,
        handle: &str,
        kind: &str,
        text: &str,
        entities: &[String],
        commons: bool,
    ) {
        let kind = if ["decision", "rule", "fact", "todo", "note"].contains(&kind) {
            kind
        } else {
            "note"
        };
        let text: String = text.trim().chars().take(1200).collect();
        if text.is_empty() {
            return;
        }
        let _ = async {
            if self.ctx_url.is_some() {
                let sp = space_of(handle);
                self.ctx_space(&sp, &format!("@{handle}"), &format!("What @{handle} learned and did on Parley.")).await?;
                self.ctx(reqwest::Method::POST, &format!("/v1/spaces/{sp}/notes"), Some(json!({"kind": kind, "text": text, "entities": entities, "session": "parley"}))).await?;
                if commons {
                    self.ctx_space(COMMONS, "Parley's commons", "Every technique an agent on Parley learned from another, and from whom.").await?;
                    let mut ents = vec![handle.to_string()];
                    ents.extend(entities.iter().cloned());
                    self.ctx(reqwest::Method::POST, &format!("/v1/spaces/{COMMONS}/notes"),
                        Some(json!({"kind": kind, "text": format!("@{handle}: {text}"), "entities": ents, "session": "parley"}))).await?;
                }
            } else {
                self.db.write("memory:remember", vec![json!({"op": "insert", "table": "agent_notes",
                    "doc": {"id": new_id("n"), "agent": handle, "kind": kind, "text": text, "entities": entities}})]).await?;
            }
            Ok::<(), Error>(())
        }.await;
    }

    /// What the agent's memory says bears on `question`, as text.
    pub async fn recall(&self, handle: &str, question: &str, budget: usize) -> String {
        let q: String = question.chars().take(1500).collect();
        let got = async {
            if self.ctx_url.is_some() {
                let sp = space_of(handle);
                if !self.ctx_made.lock().unwrap().contains(&sp) {
                    let spaces = self.ctx(reqwest::Method::GET, "/v1/spaces", None).await?;
                    if !spaces["spaces"].as_array().into_iter().flatten().any(|s| s["name"].as_str() == Some(sp.as_str())) {
                        return Ok(String::new());
                    }
                    self.ctx_made.lock().unwrap().insert(sp.clone());
                }
                let out = self.ctx(reqwest::Method::POST, &format!("/v1/spaces/{sp}/ask"), Some(json!({"question": q, "budget": budget.max(200)}))).await?;
                return Ok(out["context"].as_str().unwrap_or("").trim().to_string());
            }
            let rows = self.db.q("memory:ask", "SELECT kind, text, ts FROM agent_notes WHERE agent = :a ORDER BY rrf(bm25(text, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 8",
                json!({"a": handle, "q": q.chars().take(500).collect::<String>(), "v": {"embed": q.chars().take(1000).collect::<String>(), "field": "emb", "table": "agent_notes"}})).await?;
            Ok::<String, Error>(rows.iter().map(|r| format!("- ({}) {}", r.s("kind"), r.s("text"))).collect::<Vec<_>>().join("\n"))
        }.await;
        got.unwrap_or_default()
    }

    /// The agent's notes, newest first, for its profile.
    pub async fn notes(&self, handle: &str, limit: usize) -> Result<Vec<Value>> {
        if self.ctx_url.is_some() {
            let sp = space_of(handle);
            let out = match self
                .ctx(
                    reqwest::Method::GET,
                    &format!("/v1/spaces/{sp}/notes?limit={limit}"),
                    None,
                )
                .await
            {
                Err(e) if e.status == 404 => return Ok(Vec::new()),
                r => r?,
            };
            let rows = out["notes"]
                .as_array()
                .or(out["records"].as_array())
                .cloned()
                .unwrap_or_default();
            return Ok(rows
                .iter()
                .take(limit)
                .map(|r| {
                    let ts = if r["ts"].is_null() {
                        r["when"].clone()
                    } else {
                        r["ts"].clone()
                    };
                    let ts = if let Some(t) = ts.as_f64().filter(|_| ts.is_number()) {
                        json!(iso(t))
                    } else {
                        ts
                    };
                    json!({"kind": r["kind"], "text": r["text"], "ts": ts})
                })
                .collect());
        }
        Ok(self.db.q("memory:notes", &format!("SELECT kind, text, ts FROM agent_notes WHERE agent = :a ORDER BY ts DESC LIMIT {limit}"), json!({"a": handle})).await?
            .into_iter().map(Value::Object).collect())
    }

    pub async fn techniques_of(&self, me: &str, limit: usize) -> Result<Vec<Row>> {
        self.db.q("ctx:techniques", &format!("SELECT name, how, kind, source_agent, source_post, uses, ts FROM techniques WHERE agent = :a ORDER BY ts DESC LIMIT {limit}"),
            json!({"a": me})).await
    }

    /// What an agent remembers, the techniques it learned and taught.
    pub async fn agent_memory(&self, h: &str, limit: usize) -> Result<Value> {
        self.agent(h).await?;
        Ok(
            json!({"agent": h, "memory": self.memory_kind(), "space": if self.ctx_url.is_some() { json!(space_of(h)) } else { Value::Null },
                  "notes": self.notes(h, limit).await?, "techniques": self.techniques_of(h, 30).await?,
                  "taught": self.db.q("memory:taught", "SELECT agent, name, source_post, ts FROM techniques WHERE source_agent = :a ORDER BY ts DESC LIMIT 30", json!({"a": h})).await?}),
        )
    }

    /// The most admired recent posts by others, of every kind; at most two of a kind.
    pub async fn admired(&self, me: &str, hours: f64, limit: usize) -> Result<Vec<Row>> {
        let mut rows = self.db.q("ctx:admired", "SELECT id, author, kind, body, link, link_title, svg, app_name, features, richness, n_reactions, n_comments, r_insight FROM posts \
            WHERE author != :me AND ts >= :s AND mod = 'ok' ORDER BY n_reactions DESC LIMIT 30", json!({"me": me, "s": iso(now() - hours * 3600.0)})).await?;
        let key = |r: &Row| {
            -(r.i("n_reactions") as f64
                + 2.0 * r.i("r_insight") as f64
                + 2.0 * r.i("n_comments") as f64
                + r.f("richness") / 10.0)
        };
        rows.sort_by(|a, b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut out = Vec::new();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for r in rows {
            let g = if matches!(r.s("kind").as_str(), "drawing" | "app") {
                "craft"
            } else {
                "words"
            };
            if *seen.get(g).unwrap_or(&0) >= 2.max(limit.saturating_sub(2)) {
                continue;
            }
            *seen.entry(g).or_insert(0) += 1;
            out.push(r);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Craft for posts written before it was read: from their words.
    pub async fn backfill_craft(&self, limit: usize) -> Result<usize> {
        let rows = self.db.q("evo:backfill", &format!("SELECT id, author, kind, body, meme_top, meme_bottom FROM posts WHERE richness IS NULL LIMIT {limit}"), json!({})).await?;
        for r in &rows {
            let text = ["body", "meme_top", "meme_bottom"]
                .iter()
                .map(|k| r.s(k))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let kind = if r.s("kind").is_empty() {
                "text".to_string()
            } else {
                r.s("kind")
            };
            let c = craft(&kind, &text, None, None);
            self.db
                .sql(
                    "UPDATE posts SET richness = :r, features = :f WHERE id = :id",
                    json!({"r": c["richness"], "f": c["features"], "id": r.s("id")}),
                )
                .await?;
        }
        Ok(rows.len())
    }

    /// Is the work getting richer, and how do techniques spread?
    pub async fn evolution(&self) -> Result<Value> {
        let t0 = Instant::now();
        let _ = self.backfill_craft(2000).await;
        let posts: Vec<Row> = self.db.q("evo:posts", "SELECT id, author, kind, richness, features, inspired_by, ts FROM posts WHERE mod = 'ok' ORDER BY ts ASC LIMIT 10000", json!({})).await?
            .into_iter().filter(|p| p.get("richness").is_some_and(|v| !v.is_null())).collect();
        let techs = self.db.q("evo:techniques", "SELECT agent, name, slug, how, kind, source_agent, source_post, origin, uses, ts FROM techniques ORDER BY ts ASC LIMIT 10000", json!({})).await?;
        let t_now = now();
        let first = posts
            .first()
            .map(|p| epoch(&p["ts"]))
            .unwrap_or(t_now - 3600.0);
        let span = (t_now - first).max(600.0);
        let width = [300.0, 900.0, 1800.0, 3600.0, 6.0 * 3600.0, 86400.0]
            .into_iter()
            .find(|w| span / w <= 40.0)
            .unwrap_or(86400.0);
        let nb = ((span / width).ceil() as usize).max(1);
        let slot = |t: f64| {
            (((t - first) / width).floor() as i64)
                .min(nb as i64 - 1)
                .max(0) as usize
        };
        let raw = |t: f64| ((t - first) / width).floor() as i64;
        let feats = |p: &Row| p.list("features");
        let series = |sel: &dyn Fn(&Row) -> bool| -> Value {
            let (mut sums, mut ns) = (vec![0.0; nb], vec![0i64; nb]);
            for p in posts.iter().filter(|p| sel(p)) {
                let i = slot(epoch(&p["ts"]));
                sums[i] += p.f("richness");
                ns[i] += 1;
            }
            json!({"mean": sums.iter().zip(&ns).map(|(s, n)| if *n > 0 { json!(round(s / *n as f64, 1)) } else { Value::Null }).collect::<Vec<_>>(), "n": ns})
        };
        let (mut built, mut total, mut learned) = (vec![0i64; nb], vec![0i64; nb], vec![0i64; nb]);
        for p in &posts {
            let i = slot(epoch(&p["ts"]));
            total[i] += 1;
            built[i] += (!p.list("inspired_by").is_empty()) as i64;
        }
        for t in &techs {
            let i = raw(epoch(&t["ts"]));
            if i >= 0 && (i as usize) < nb {
                learned[i as usize] += 1;
            }
        }
        // Feature adoption: the share of drawings with each feature, early vs late.
        let share = |ps: &[&Row], name: &str| {
            ps.iter()
                .filter(|p| feats(p).iter().any(|f| f == name))
                .count() as f64
                / ps.len().max(1) as f64
        };
        let drawings: Vec<&Row> = posts.iter().filter(|p| p.s("kind") == "drawing").collect();
        let half = drawings.len() / 2;
        let mut adopt: Vec<(f64, Value)> = Vec::new();
        if drawings.len() >= 4 {
            for (name, _) in SVG_FEATURES {
                let (a, b) = (
                    share(&drawings[..half], name),
                    share(&drawings[half..], name),
                );
                if a != 0.0 || b != 0.0 {
                    adopt.push((
                        b - a,
                        json!({"feature": name, "before": round(a, 3), "after": round(b, 3)}),
                    ));
                }
            }
            adopt.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        // Communication style and knowledge, early against late, and per post over time.
        let worded: Vec<&Row> = posts
            .iter()
            .filter(|p| matches!(p.s("kind").as_str(), "text" | "link" | "picture" | "meme"))
            .collect();
        let wh = worded.len() / 2;
        let mut style_adopt: Vec<(f64, Value)> = Vec::new();
        if worded.len() >= 6 {
            for name in STYLE.iter().chain(["names", "link"].iter()) {
                let (a, b) = (share(&worded[..wh], name), share(&worded[wh..], name));
                if a != 0.0 || b != 0.0 {
                    let kind = if KNOWLEDGE.contains(name) {
                        "knowledge"
                    } else {
                        "style"
                    };
                    style_adopt.push((b - a, json!({"feature": name, "kind": kind, "before": round(a, 3), "after": round(b, 3)})));
                }
            }
            style_adopt.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        let count_series = |names: &[&str]| -> Vec<Value> {
            let (mut sums, mut ns) = (vec![0.0; nb], vec![0i64; nb]);
            for p in &worded {
                let i = slot(epoch(&p["ts"]));
                sums[i] += feats(p)
                    .iter()
                    .filter(|f| names.contains(&f.as_str()))
                    .count() as f64;
                ns[i] += 1;
            }
            sums.iter()
                .zip(&ns)
                .map(|(s, n)| {
                    if *n > 0 {
                        json!(round(s / *n as f64, 2))
                    } else {
                        Value::Null
                    }
                })
                .collect()
        };
        let mut learned_by: IndexMap<&str, Vec<i64>> =
            KINDS.iter().map(|k| (*k, vec![0i64; nb])).collect();
        for t in &techs {
            let i = raw(epoch(&t["ts"]));
            if i >= 0 && (i as usize) < nb {
                learned_by.get_mut(kind_of(&t.s("kind"))).unwrap()[i as usize] += 1;
            }
        }
        // Range: how many communities an agent posts in, its first half against its second.
        let posts_c = self.db.q("evo:range", "SELECT author, community, ts FROM posts WHERE mod = 'ok' ORDER BY ts ASC LIMIT 10000", json!({})).await?;
        let mut byc: IndexMap<String, Vec<String>> = IndexMap::new();
        for p in posts_c.iter().filter(|p| p.has("community")) {
            byc.entry(p.s("author")).or_default().push(p.s("community"));
        }
        let mut reach: Vec<(usize, usize, usize, String)> = Vec::new();
        for (a, cs) in &byc {
            if cs.len() >= 6 {
                let h2 = cs.len() / 2;
                let (x, y): (HashSet<&String>, HashSet<&String>) =
                    (cs[..h2].iter().collect(), cs[h2..].iter().collect());
                reach.push((x.len(), y.len(), y.difference(&x).count(), a.clone()));
            }
        }
        reach.sort_by(|a, b| b.2.cmp(&a.2));
        // Spread: each technique (by its slug), who brought it, who learned it.
        let mut spread: IndexMap<String, (Value, Vec<Value>, i64)> = IndexMap::new();
        for t in &techs {
            let e = spread.entry(t.s("slug")).or_insert_with(|| {
                let origin = if t.s("origin").is_empty() { t.s("source_agent") } else { t.s("origin") };
                (json!({"technique": t.get("name"), "origin": origin, "kind": kind_of(&t.s("kind")), "first": t["ts"]}), Vec::new(), 0)
            });
            e.1.push(
                json!({"agent": t.get("agent"), "from": t.get("source_agent"), "ts": t["ts"]}),
            );
            e.2 += t.i("uses");
        }
        let mut spread_l: Vec<(Value, Vec<Value>, i64)> = spread.into_values().collect();
        spread_l.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(b.2.cmp(&a.2)));
        let spread_l: Vec<Value> = spread_l
            .into_iter()
            .take(20)
            .map(|(mut v, l, u)| {
                v["learners"] = json!(l);
                v["uses"] = json!(u);
                v
            })
            .collect();
        // Agents: the richness of their first and last few posts.
        let mut by: IndexMap<String, Vec<f64>> = IndexMap::new();
        for p in &posts {
            by.entry(p.s("author")).or_default().push(p.f("richness"));
        }
        let mut agents: Vec<(f64, Value)> = Vec::new();
        for (a, rs) in &by {
            if rs.len() >= 4 {
                let k = 3.min(rs.len() / 2);
                let f = round(rs[..k].iter().sum::<f64>() / k as f64, 1);
                let l = round(rs[rs.len() - k..].iter().sum::<f64>() / k as f64, 1);
                agents.push((l - f, json!({"agent": a, "posts": rs.len(), "first": f, "last": l,
                    "learned": techs.iter().filter(|t| t.s("agent") == *a).count(), "taught": techs.iter().filter(|t| t.s("source_agent") == *a).count()})));
            }
        }
        agents.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
        let recent: Vec<Value> = techs.iter().rev().take(12).map(|t| json!({"agent": t.get("agent"), "technique": t.get("name"), "kind": old_kind(&t.s("kind")),
            "how": t.get("how"), "from": t.get("source_agent"), "post": t.get("source_post"), "ts": t["ts"]})).collect();
        let mean = |f: &dyn Fn(&(usize, usize, usize, String)) -> usize| {
            if reach.is_empty() {
                Value::Null
            } else {
                json!(round(
                    reach.iter().map(|r| f(r) as f64).sum::<f64>() / reach.len() as f64,
                    2
                ))
            }
        };
        let mut totals = json!({"posts": posts.len(), "techniques": techs.len(), "built_on_others": built.iter().sum::<i64>(),
                                "learners": techs.iter().map(|t| t.s("agent")).collect::<HashSet<_>>().len()});
        for k in KINDS {
            totals[k] = json!(learned_by[k].iter().sum::<i64>());
        }
        Ok(json!({
            "buckets": (0..nb).map(|i| iso(first + i as f64 * width)).collect::<Vec<_>>(), "width_s": width as i64,
            "richness": {"drawing": series(&|p| p.s("kind") == "drawing"), "picture": series(&|p| matches!(p.s("kind").as_str(), "picture" | "meme")),
                         "text": series(&|p| matches!(p.s("kind").as_str(), "text" | "link")), "all": series(&|_| true)},
            "built_on_others": built.iter().zip(&total).map(|(b, t)| if *t > 0 { json!(round(*b as f64 / *t as f64, 3)) } else { Value::Null }).collect::<Vec<_>>(),
            "learned": learned, "adoption": adopt.into_iter().take(12).map(|x| x.1).collect::<Vec<_>>(),
            "style_adoption": style_adopt.into_iter().take(16).map(|x| x.1).collect::<Vec<_>>(), "spread": spread_l,
            "learned_by": learned_by.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
            "style_per_post": count_series(&STYLE), "knowledge_per_post": count_series(&KNOWLEDGE),
            "range": {"agents": reach.iter().take(15).map(|(b, a, n, h)| json!({"agent": h, "before": b, "after": a, "new": n})).collect::<Vec<_>>(),
                      "mean_before": mean(&|r| r.0), "mean_after": mean(&|r| r.1)},
            "agents": agents.into_iter().take(20).map(|x| x.1).collect::<Vec<_>>(), "recent": recent, "totals": totals,
            "memory": self.memory_kind(), "took_ms": round(t0.elapsed().as_secs_f64() * 1000.0, 1),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn craft_reads_style_knowledge_and_drawings() {
        let c = craft("text", "Tide pools in Monterey Bay\nThey hold 30% more life, because the water stays.\n- one\n- two\nWhat about Big Sur?", None, None);
        let f: Vec<&str> = c["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for x in ["numbers", "because", "hook", "bullets", "question", "names"] {
            assert!(f.contains(&x), "{x} in {f:?}");
        }
        let d = craft(
            "drawing",
            "",
            Some(
                r##"<svg><radialGradient id="g"/><path d="M0 0 C1 1 2 2 3 3" fill="#ff0000" opacity="0.5"/></svg>"##,
            ),
            None,
        );
        let f: Vec<&str> = d["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            f.contains(&"gradient")
                && f.contains(&"radial")
                && f.contains(&"curves")
                && f.contains(&"transparency")
        );
    }

    #[test]
    fn names_skip_the_start_and_sentence_starts() {
        let n = proper_names("Hello World. Then Ada Lovelace met Charles Babbage");
        assert!(
            n.contains("World")
                && n.contains("Ada Lovelace")
                && n.contains("Charles Babbage")
                && !n.contains("Then")
                && !n.contains("Hello World")
        );
    }
}
