//! What a language model writes, through ChloeDB's `llm()` (the server
//! holds the provider's key; the app never does): a topic summed up from
//! its papers with each claim cited, a paper's TL;DR, and the paragraphs
//! of a digest. Each is kept (`ax_answers`, `ax_notes`), so it is written
//! once and costs nothing the next time.

use serde_json::{json, Map, Value};

use crate::search::{self, Ctx};
use crate::strata::{Row, RowExt};
use crate::understand::{self, Parsed};

pub const SUMMARY: &str = "You write the state of research on a topic for a busy researcher, using only the numbered arXiv abstracts you are given. \
Start with two or three sentences on what the topic is and where it stands now. Then 4 to 6 bullets: the main approaches and how they differ, the strongest results with their numbers, \
what changed most recently, and the open problems. Cite every claim with the numbers of the abstracts it comes from, like [3] or [2][7]. Never cite a number you were not given. \
Keep each bullet under 45 words. Plain, exact language; no hype, no preamble, no closing summary. If the abstracts do not cover the topic, say so in one sentence.";

pub const TLDR: &str = "You write a TL;DR of an arXiv paper from its title and abstract for a researcher deciding whether to read it. \
Answer with JSON only: {\"tldr\": \"one sentence of at most 30 words: what it does and its main result, with a number if the abstract gives one\", \
\"why\": \"one sentence of at most 25 words on why it matters or who should read it\", \"method\": \"at most 8 words naming the method\", \"results\": [\"up to 3 short results with their numbers\"]}";

pub fn key(q: &str) -> String {
    let words: Vec<String> = q.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect();
    format!("summary:{}", words.join(" "))
}

/// The abstracts a summary is written from: words and meaning fused,
/// citations and recency as a prior, packed into the budget by the server
/// (near duplicates skipped, long abstracts excerpted).
pub async fn sources(c: &Ctx<'_>, p: &Parsed, budget: usize) -> Vec<Row> {
    let mut params = Map::new();
    let conds = search::filters(p, &mut params);
    if !search::word_params(p, &mut params) {
        return Vec::new();
    }
    let wh = search::where_of(&conds, "");
    let sql = format!(
        "SELECT id, title, abstract, authors, nauthors, year, cites FROM ax_papers {wh} \
         ORDER BY rrf(bm25(title, :q, synonyms => 1), bm25(abstract, :q, synonyms => 1, min => '60%'), similarity(emb, :v)) * (1 + 0.15 * pop) * (0.7 + 0.3 * decay(created, now(), 94608000)) DESC \
         LIMIT 80 PACK {budget} TOKENS OF abstract EXCERPT 260 DEDUPE 0.85 WITH RECEIPT"
    );
    let rows = c.soft("the abstracts to write from (packed)", &sql, Value::Object(params)).await;
    rows.into_iter().filter(|r| !r.s("pack_decision").starts_with("skip")).collect()
}

fn context(rows: &[Row]) -> String {
    let mut s = String::new();
    for (i, r) in rows.iter().enumerate() {
        s.push_str(&format!("[{}] “{}” ({}, {}, {} citations):\n{}\n\n", i + 1, r.s("title"), crate::answer::first_author(r), r.i("year"), r.i("cites"), r.s("abstract").trim()));
    }
    s
}

pub async fn llm(c: &Ctx<'_>, what: &str, model: &str, instructions: &str, question: &str, context: &str, max_tokens: usize) -> Result<String, String> {
    // The model is a literal of the statement (llm() takes no parameter there).
    if model.is_empty() || !model.chars().all(|ch| ch.is_ascii_alphanumeric() || "/._-:".contains(ch)) {
        return Err(format!("not a model name: {model:?}"));
    }
    // llm() answers for each row a query returns: one row of the graph's nodes.
    let sql = format!("SELECT id, llm('{model}', :instructions, :question, :context, max_tokens => {max_tokens}) AS answer FROM ax_nodes LIMIT 1");
    let got = c.grows(what, &sql, json!({"instructions": instructions, "question": question, "context": context})).await.map_err(|e| e.to_string())?;
    Ok(got.first().map(|r| r.s("answer")).unwrap_or_default())
}

/// A topic summed up from its papers, kept or written now.
pub async fn summary(c: &Ctx<'_>, q: &str, model: &str, fresh: bool) -> Result<Value, String> {
    let k = key(q);
    if !fresh {
        let kept = c.gsoft("a kept summary", "SELECT answer, sources, model, at, ms FROM ax_answers WHERE id = :k", json!({"k": k})).await;
        if let Some(r) = kept.first() {
            let sources: Value = serde_json::from_str(&r.s("sources")).unwrap_or(Value::Null);
            return Ok(json!({"question": q, "answer": r.s("answer"), "sources": sources, "model": r.s("model"), "at": r.s("at"), "kept": true, "ms": r.f("ms")}));
        }
    }
    let mut p = understand::parse(q, chrono::Utc::now().timestamp());
    p.ask = None;
    let t0 = std::time::Instant::now();
    let taken = sources(c, &p, 7000).await;
    if taken.is_empty() {
        return Ok(json!({"question": q, "answer": "", "sources": [], "note": "No papers speak to this."}));
    }
    let topic = crate::answer::subject(&p);
    let answer = llm(c, "the summary (llm() on the server)", model, SUMMARY, &format!("Topic: {topic}"), &format!("Abstracts:\n\n{}", context(&taken)), 1100).await?;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let sources: Vec<Value> = taken.iter().enumerate().map(|(i, r)| json!({"n": i + 1, "id": r.s("id"), "title": r.s("title"), "authors": crate::answer::first_author(r), "year": r.i("year"), "cites": r.i("cites")})).collect();
    let at = chrono::Utc::now().to_rfc3339();
    let _ = c.g.upsert("ax_answers", vec![json!({"id": k, "kind": "summary", "question": q, "answer": answer, "sources": serde_json::to_string(&sources).unwrap_or_default(), "model": model, "at": at, "ms": ms})]).await;
    Ok(json!({"question": q, "answer": answer, "sources": sources, "model": model, "at": at, "kept": false, "ms": ms}))
}

/// A paper's TL;DR, kept or written now.
pub async fn tldr(c: &Ctx<'_>, id: &str, model: &str) -> Result<Value, String> {
    let id = understand::clean_id(id);
    let k = format!("tldr:{id}");
    let kept = c.gsoft("a kept TL;DR", "SELECT text, model, at FROM ax_notes WHERE id = :k", json!({"k": k})).await;
    if let Some(r) = kept.first() {
        let v: Value = serde_json::from_str(&r.s("text")).unwrap_or(json!({"tldr": r.s("text")}));
        return Ok(json!({"id": id, "note": v, "model": r.s("model"), "kept": true}));
    }
    let row = c.soft("the paper", "SELECT id, title, abstract FROM ax_papers WHERE id = :id", json!({"id": id})).await.into_iter().next().ok_or("no such paper")?;
    let t0 = std::time::Instant::now();
    let text = llm(c, "the TL;DR (llm() on the server)", model, TLDR, &format!("Title: {}", row.s("title")), &format!("Abstract: {}", row.s("abstract")), 400).await?;
    let v = json_in(&text).unwrap_or(json!({"tldr": text.trim()}));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": "tldr", "text": v.to_string(), "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": ms})]).await;
    Ok(json!({"id": id, "note": v, "model": model, "kept": false, "ms": ms}))
}

/// The first JSON object in a model's answer.
pub fn json_in(text: &str) -> Option<Value> {
    let a = text.find('{')?;
    let b = text.rfind('}')?;
    serde_json::from_str(text.get(a..=b)?).ok()
}
