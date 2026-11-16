//! Standing questions: "tell me when someone uses diffusion models for
//! weather forecasting". A question is kept with its reader. Every hour the
//! papers of the last week are searched for it (all of its words and most
//! of them by BM25 over titles and abstracts, and its meaning, fused by
//! rank), and those of the fifteen closest not read for it yet are read by
//! the model against the question, all in one call. The papers that answer
//! it are kept with a sentence on why, newest first, as a page and an RSS
//! feed; those that do not are kept too, so no paper is read twice for one
//! question, and a round where the closest papers are all read calls no
//! model.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::{Row, RowExt};

/// The closest papers of a window read for a question.
pub const CLOSEST: usize = 15;
/// Questions a reader may keep, and questions in all.
pub const PER_READER: usize = 10;
pub const IN_ALL: usize = 2000;

const FILLER: &[&str] = &[
    "tell", "me", "when", "whenever", "someone", "somebody", "anyone", "anybody", "let", "know", "notify", "alert", "if", "new", "paper", "papers", "a", "an",
    "the", "any", "about", "show", "find", "i", "want", "to", "see", "there", "is", "are", "publishes", "publish", "posts", "writes", "write", "work", "works",
    "uses", "use", "using", "applies", "apply", "applying", "on", "of", "that", "which", "who", "please", "every", "time",
];

/// The words of a question to search by: the question without the
/// phrases of asking ("tell me when someone uses").
pub fn search_words(question: &str) -> String {
    let words: Vec<&str> = question.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '+' || c == '.')).map(|w| w.trim_matches('.')).filter(|w| !w.is_empty()).collect();
    let kept: Vec<&str> = words.iter().copied().filter(|w| !FILLER.contains(&w.to_lowercase().as_str())).collect();
    if kept.is_empty() { words.join(" ") } else { kept.join(" ") }
}

/// Reciprocal rank fusion of ranked id lists.
pub fn fuse(lists: &[Vec<String>]) -> Vec<String> {
    let mut score: HashMap<&str, f64> = HashMap::new();
    for l in lists {
        for (i, id) in l.iter().enumerate() {
            *score.entry(id.as_str()).or_default() += 1.0 / (60.0 + i as f64);
        }
    }
    let mut ids: Vec<(&str, f64)> = score.into_iter().collect();
    ids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(b.0)));
    ids.into_iter().map(|(id, _)| id.to_string()).collect()
}

const JUDGE: &str = "A reader asked to be told about new papers that answer a standing question. Read each numbered paper and decide whether it is one the reader asked for: it must do what the question describes, not merely use the same words or touch the area. Answer with JSON only, one object per paper in order: [{\"n\": 1, \"fit\": true, \"why\": \"one sentence of at most 30 words on what the paper does that answers the question, or why it does not\"}]";

/// The model's verdicts, by the papers' numbers (from 1). Each object is
/// read on its own, so an answer cut short keeps the verdicts it holds.
pub fn verdicts(text: &str) -> HashMap<usize, (bool, String)> {
    let mut out = HashMap::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_str = false;
    let mut esc = false;
    for (i, ch) in text.char_indices() {
        if in_str {
            match (esc, ch) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(v) = start.and_then(|s| serde_json::from_str::<Value>(&text[s..=i]).ok()) {
                        if let Some(n) = v["n"].as_u64() {
                            out.insert(n as usize, (v["fit"].as_bool().unwrap_or(false), v["why"].as_str().unwrap_or("").trim().to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// What one check found: papers read and those that answer.
pub struct Checked {
    pub read: usize,
    pub fit: usize,
}

/// Checks one question against the papers since `since` (an RFC 3339 time):
/// the `closest` papers not read for it yet, read by the model in one call
/// when `allow` lets it (the day's allowance of model calls).
pub async fn check(c: &Ctx<'_>, model: &str, watch: &str, question: &str, since: &str, closest: usize, allow: &(dyn Fn() -> Result<(), String> + Sync)) -> Result<Checked, String> {
    let words = search_words(question);
    let params = json!({
        "q": words, "m": words, "min": crate::search::min_words(&words), "since": since,
        "v": {"embed": words, "field": "emb", "table": "ax_papers"},
    });
    // Papers with all its words (up to six) count in two lists of three.
    let mut all = params.clone();
    all["min"] = json!(words.split_whitespace().count().clamp(1, 6).to_string());
    let rank = "ORDER BY coalesce(bm25(title, :q, synonyms => 1), 0) + 0.5 * coalesce(bm25(abstract, :q, synonyms => 1), 0) DESC LIMIT 30";
    let (by_all, by_words, by_meaning) = tokio::join!(
        c.soft("papers with all its words", &format!("SELECT id FROM ax_papers WHERE created >= :since AND {} {rank}", crate::search::MATCHED), all),
        c.soft("papers with most of its words", &format!("SELECT id FROM ax_papers WHERE created >= :since AND {} {rank}", crate::search::MATCHED), params.clone()),
        c.soft("papers by its meaning", "SELECT id FROM ax_papers WHERE created >= :since ORDER BY similarity(emb, :v) DESC LIMIT 30", params.clone()),
    );
    let ids = |rows: &[Row]| rows.iter().map(|r| r.s("id")).collect::<Vec<_>>();
    let top: Vec<String> = fuse(&[ids(&by_all), ids(&by_words), ids(&by_meaning)]).into_iter().take(closest).collect();
    if top.is_empty() {
        return Ok(Checked { read: 0, fit: 0 });
    }
    let keys: Vec<String> = top.iter().map(|p| format!("{watch}|{p}")).collect();
    let read: HashSet<String> = c.gsoft("the papers read for it", "SELECT paper FROM ax_matches WHERE id IN (:k)", json!({"k": keys})).await.into_iter().map(|r| r.s("paper")).collect();
    let fresh: Vec<String> = top.into_iter().filter(|p| !read.contains(p)).collect();
    if fresh.is_empty() {
        return Ok(Checked { read: 0, fit: 0 });
    }
    let rows = c.soft("the papers to read", &format!("SELECT {COLS}, abstract FROM ax_papers WHERE id IN (:ids)"), json!({"ids": fresh})).await;
    let by_id: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    let papers: Vec<&Row> = fresh.iter().filter_map(|id| by_id.get(id)).collect();
    allow()?;
    let mut ctx = String::new();
    for (i, r) in papers.iter().enumerate() {
        ctx.push_str(&format!("[{}] “{}” ({})\n{}\n\n", i + 1, r.s("title"), r.s("pcat"), r.s("abstract").trim()));
    }
    let text = crate::ask::llm(c, "which answer the question (llm() on the server)", model, JUDGE, &format!("The standing question: {question}"), &ctx, 400 + 120 * papers.len()).await?;
    let v = verdicts(&text);
    let now = chrono::Utc::now().to_rfc3339();
    let mut out = Vec::new();
    let mut fit = 0;
    for (i, r) in papers.iter().enumerate() {
        // A paper the model left out is read again next time.
        let Some((ok, why)) = v.get(&(i + 1)) else { continue };
        fit += *ok as usize;
        out.push(json!({"id": format!("{watch}|{}", r.s("id")), "watch": watch, "paper": r.s("id"), "fit": ok, "why": why, "created": r.s("created"), "at": now}));
    }
    let n = out.len();
    if !out.is_empty() {
        c.g.upsert("ax_matches", out).await.map_err(|e| e.to_string())?;
    }
    Ok(Checked { read: n, fit })
}

/// A question's answers as RSS 2.0, newest first.
pub fn rss(watch: &str, question: &str, matches: &[Row], base: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel><title>{}</title><link>{base}/#/watch/{watch}</link><description>New arXiv papers that answer this question, read by a model as they arrive (arXiv Explorer on ChloeDB)</description>\n",
        esc(question)
    );
    for r in matches {
        let id = r.s("paper");
        let date = chrono::DateTime::parse_from_rfc3339(&r.s("created")).map(|d| d.to_rfc2822()).unwrap_or_default();
        let html = format!("<p>{}</p><p>{}</p><p><a href=\"https://arxiv.org/abs/{id}\">arXiv:{id}</a> · <a href=\"{base}/#/paper/{id}\">in arXiv Explorer</a></p>", esc(&r.s("why")), esc(&r.s("authors")));
        s.push_str(&format!("<item><title>{}</title><guid isPermaLink=\"false\">{}</guid><link>https://arxiv.org/abs/{id}</link><pubDate>{date}</pubDate><description>{}</description></item>\n", esc(&r.s("title")), esc(&format!("{watch}|{id}")), esc(&html)));
    }
    s.push_str("</channel></rss>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_searched_by_what_it_is_about() {
        assert_eq!(search_words("Tell me when someone uses diffusion models for weather forecasting"), "diffusion models for weather forecasting");
        assert_eq!(search_words("new papers on RL for LLM agents?"), "RL for LLM agents");
        assert_eq!(search_words("tell me when"), "tell me when");
    }

    #[test]
    fn ranks_fuse_and_verdicts_parse() {
        let f = fuse(&[vec!["a".into(), "b".into(), "c".into()], vec!["c".into(), "a".into()]]);
        assert_eq!(f[0], "a");
        assert_eq!(f.len(), 3);
        let v = verdicts("Here: [{\"n\": 1, \"fit\": true, \"why\": \"Uses a diffusion model to forecast rain.\"}, {\"n\": 2, \"fit\": false, \"why\": \"About images.\"}]");
        assert_eq!(v[&1], (true, "Uses a diffusion model to forecast rain.".to_string()));
        assert!(!v[&2].0);
        assert!(verdicts("no json").is_empty());
        // Cut short: the whole objects stay.
        let v = verdicts("[{\"n\": 1, \"fit\": true, \"why\": \"A {curly} one.\"}, {\"n\": 2, \"fit\": fa");
        assert_eq!(v.len(), 1);
        assert_eq!(v[&1].1, "A {curly} one.");
    }
}
