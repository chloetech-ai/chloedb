//! "What does HN think about …": the comments that speak to a question,
//! packed into a model's budget by the database (`PACK n TOKENS`, near
//! duplicates skipped, long comments excerpted to their best stretch),
//! then summed up by a language model through ChloeDB's `llm()` with each
//! claim cited to a numbered comment. The model's key lives with the
//! server; the app never holds it. Answers are kept (`hn_answers`), so a
//! question asked again costs nothing.

use serde_json::{json, Value};

use crate::query::{self, Parsed};
use crate::search::Ctx;
use crate::strata::{Row, RowExt};

pub const INSTRUCTIONS: &str = "You sum up what Hacker News commenters think about a question, using only the numbered comments you are given. \
Write a short answer for a busy engineer: one sentence with the overall take, then 3 to 6 bullets with the main views, where people agree, where they disagree, and concrete experiences or numbers. \
Cite every claim with the numbers of the comments it comes from, like [3] or [2][7]. Never cite a number you were not given. \
Keep each bullet under 40 words. Plain language, no hype, no preamble. Attribute opinions to commenters (\"several commenters\", \"one who ran it in production\"), not to yourself. \
If the comments do not address the question, say so in one sentence.";

pub fn key(q: &str) -> String {
    let words: Vec<String> = q.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect();
    format!("ask:{}", words.join(" "))
}

/// The comments the answer is made from, with the rows the pack skipped.
pub async fn sources(c: &Ctx<'_>, p: &Parsed, budget: usize) -> (Vec<Row>, Vec<Row>) {
    let words = query::plain_words(p);
    let mut params = serde_json::Map::new();
    let conds = crate::search::filters(p, false, &mut params);
    params.insert("q".into(), json!(words));
    params.insert("m".into(), json!(query::match_text(p)));
    params.insert("v".into(), json!({"embed": words, "field": "emb", "table": "hn_comments"}));
    let extra = if conds.is_empty() { String::new() } else { format!(" AND {}", conds.join(" AND ")) };
    // Words and meaning fused, comments that started a discussion first among equals.
    let sql = format!(
        "SELECT id, author, year, story, story_title, body FROM hn_comments WHERE chars > 120{extra} \
         ORDER BY rrf(bm25(body, :q, synonyms => 1, min => '50%'), similarity(emb, :v), bm25(story_title, :q)) * (1 + 0.2 * pop) DESC \
         LIMIT 120 PACK {budget} TOKENS OF body EXCERPT 220 DEDUPE 0.8 WITH RECEIPT"
    );
    let rows = match c.rows("the comments to answer from (packed)", &sql, Value::Object(params.clone())).await {
        Ok(r) => r,
        // Comments kept without vectors: by their words and their story's title.
        Err(_) => {
            let sql = format!(
                "SELECT id, author, year, story, story_title, body FROM hn_comments WHERE chars > 120{extra} \
                 ORDER BY rrf(bm25(body, :q, synonyms => 1, min => '50%'), bm25(story_title, :q)) * (1 + 0.2 * pop) DESC \
                 LIMIT 120 PACK {budget} TOKENS OF body EXCERPT 220 DEDUPE 0.8 WITH RECEIPT"
            );
            c.soft("the comments to answer from, by words (packed)", &sql, Value::Object(params)).await
        }
    };
    let (taken, skipped): (Vec<Row>, Vec<Row>) = rows.into_iter().partition(|r| !r.s("pack_decision").starts_with("skip"));
    (taken, skipped)
}

pub fn context(rows: &[Row]) -> String {
    let mut s = String::new();
    for (i, r) in rows.iter().enumerate() {
        s.push_str(&format!("[{}] {} ({}), on “{}”:\n{}\n\n", i + 1, r.s("author"), r.s("year"), r.s("story_title"), r.s("body").trim()));
    }
    s
}

/// The answer to a question, from the kept answers or made now.
pub async fn ask(c: &Ctx<'_>, q: &str, model: &str, fresh: bool) -> Result<Value, String> {
    let k = key(q);
    if !fresh {
        let kept = c.soft("a kept answer", "SELECT answer, sources, model, at, ms FROM hn_answers WHERE id = :k", json!({"k": k})).await;
        if let Some(r) = kept.first() {
            let sources: Value = serde_json::from_str(&r.s("sources")).unwrap_or(Value::Null);
            return Ok(json!({"question": q, "answer": r.s("answer"), "sources": sources, "model": r.s("model"), "at": r.s("at"), "kept": true, "ms": r.f("ms")}));
        }
    }
    let p = query::parse(q, crate::ingest::now());
    let t0 = std::time::Instant::now();
    let (taken, skipped) = sources(c, &p, 6000).await;
    if taken.is_empty() {
        return Ok(json!({"question": q, "answer": "", "sources": [], "note": "No comments speak to this yet."}));
    }
    let ctx = context(&taken);
    let sql = "SELECT llm(:model, :instructions, :question, :context, max_tokens => 900) AS answer";
    let got = c.rows("the answer (llm() on the server)", sql, json!({"model": model, "instructions": INSTRUCTIONS, "question": format!("Question: {q}"), "context": format!("Comments:\n\n{ctx}")})).await.map_err(|e| e.to_string())?;
    let answer = got.first().map(|r| r.s("answer")).unwrap_or_default();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let sources: Vec<Value> = taken.iter().enumerate().map(|(i, r)| json!({"n": i + 1, "id": r.i("id"), "author": r.s("author"), "year": r.i("year"), "story": r.i("story"), "story_title": r.s("story_title"), "text": crate::text::clip(&r.s("body"), 400), "decision": r.s("pack_decision"), "tokens": r.i("pack_tokens")})).collect();
    let receipt = json!({"taken": taken.len(), "skipped": skipped.len(), "skipped_why": skipped.iter().map(|r| r.s("pack_decision")).collect::<Vec<_>>()});
    let at = chrono::Utc::now().to_rfc3339();
    let _ = c.db.write(vec![json!({"op": "upsert", "table": "hn_answers", "doc": {"id": k, "kind": "ask", "question": q, "answer": answer, "sources": serde_json::to_string(&sources).unwrap_or_default(), "model": model, "at": at, "ms": ms}})]).await;
    Ok(json!({"question": q, "answer": answer, "sources": sources, "model": model, "at": at, "kept": false, "ms": ms, "receipt": receipt}))
}
