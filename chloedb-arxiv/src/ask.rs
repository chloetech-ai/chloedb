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

pub const NOVELTY: &str = "You tell a reviewer what an arXiv paper adds to the earlier papers closest to it. You get the paper's abstract and the numbered abstracts of up to three earlier papers. \
Answer with JSON only: {\"adds\": \"two sentences, at most 60 words: what this paper does that [1], [2] and [3] did not, naming them by number\", \
\"shares\": \"one sentence, at most 30 words: what it takes from or shares with them, by number\", \
\"kind\": \"one of: new problem, new method, better results, new setting, analysis or survey, close to earlier work\"}. \
Use only what the abstracts say. If an earlier paper does the same thing, say so plainly and choose close to earlier work.";

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
    // A model that thinks first can spend all of max_tokens thinking and
    // write no text; the server keeps that empty answer for the same
    // question, so a second try asks with room to spare (a new question).
    let mut room = max_tokens;
    for _ in 0..2 {
        let sql = format!("SELECT id, llm('{model}', :instructions, :question, :context, max_tokens => {room}) AS answer FROM ax_nodes LIMIT 1");
        let got = c.grows(what, &sql, json!({"instructions": instructions, "question": question, "context": context})).await.map_err(|e| e.to_string())?;
        let text = got.first().map(|r| r.s("answer")).unwrap_or_default();
        if !text.trim().is_empty() {
            return Ok(text);
        }
        room = (max_tokens * 4).max(3000).min(8192);
    }
    Err("the model wrote no text: try again in a minute".into())
}

/// A topic summed up from its papers, kept or written now.
/// The key a topic's summary is kept under, for a reader's level ("" for the usual one).
pub fn summary_key(q: &str, level: &str) -> String {
    if level.is_empty() { key(q) } else { format!("{}|{level}", key(q)) }
}

pub async fn summary(c: &Ctx<'_>, q: &str, model: &str, fresh: bool, level: &str) -> Result<Value, String> {
    let k = summary_key(q, level);
    if !fresh {
        let kept = c.gsoft("a kept summary", "SELECT answer, sources, model, at, ms FROM ax_answers WHERE id = :k", json!({"k": k})).await;
        // An empty one (kept before empty answers were refused) is written again.
        if let Some(r) = kept.first().filter(|r| !r.s("answer").trim().is_empty()) {
            let sources: Value = serde_json::from_str(&r.s("sources")).unwrap_or(Value::Null);
            return Ok(json!({"question": q, "level": level, "answer": r.s("answer"), "sources": sources, "model": r.s("model"), "at": r.s("at"), "kept": true, "ms": r.f("ms")}));
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
    let instructions = if level.is_empty() { SUMMARY.to_string() } else { format!("{SUMMARY} {}", level_note(level)) };
    let answer = llm(c, "the summary (llm() on the server)", model, &instructions, &format!("Topic: {topic}"), &format!("Abstracts:\n\n{}", context(&taken)), 3000).await?;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let sources: Vec<Value> = taken.iter().enumerate().map(|(i, r)| json!({"n": i + 1, "id": r.s("id"), "title": r.s("title"), "authors": crate::answer::first_author(r), "year": r.i("year"), "cites": r.i("cites")})).collect();
    let at = chrono::Utc::now().to_rfc3339();
    let _ = c.g.upsert("ax_answers", vec![json!({"id": k, "kind": "summary", "question": q, "answer": answer, "sources": serde_json::to_string(&sources).unwrap_or_default(), "model": model, "at": at, "ms": ms})]).await;
    Ok(json!({"question": q, "level": level, "answer": answer, "sources": sources, "model": model, "at": at, "kept": false, "ms": ms}))
}

/// A paper's TL;DR, kept or written now.
pub async fn tldr(c: &Ctx<'_>, id: &str, model: &str) -> Result<Value, String> {
    let id = understand::clean_id(id);
    let k = format!("tldr:{id}");
    let kept = c.gsoft("a kept TL;DR", "SELECT text, model, at FROM ax_notes WHERE id = :k", json!({"k": k})).await;
    if let Some(r) = kept.first().filter(|r| !r.s("text").trim().is_empty() && r.s("text") != r#"{"tldr":""}"#) {
        let v: Value = serde_json::from_str(&r.s("text")).unwrap_or(json!({"tldr": r.s("text")}));
        return Ok(json!({"id": id, "note": v, "model": r.s("model"), "kept": true}));
    }
    let row = c.soft("the paper", "SELECT id, title, abstract FROM ax_papers WHERE id = :id", json!({"id": id})).await.into_iter().next().ok_or("no such paper")?;
    let t0 = std::time::Instant::now();
    let text = llm(c, "the TL;DR (llm() on the server)", model, TLDR, &format!("Title: {}", row.s("title")), &format!("Abstract: {}", row.s("abstract")), 1200).await?;
    let v = json_in(&text).unwrap_or(json!({"tldr": text.trim()}));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": "tldr", "text": v.to_string(), "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": ms})]).await;
    Ok(json!({"id": id, "note": v, "model": model, "kept": false, "ms": ms}))
}

/// The readers a paper is explained for.
pub const LEVELS: &[&str] = &["student", "practitioner", "expert"];

/// How to write for a reader at a level.
pub fn level_note(level: &str) -> &'static str {
    match level {
        "student" => "The reader is a student new to the field: use plain words, define every technical term in passing, and give an everyday analogy when one makes the idea clear.",
        "practitioner" => "The reader is an engineer who might use this: say what it does, what it needs to be applied (data, compute, code), its results with their numbers, and when to choose it over the usual approach.",
        _ => "The reader is a researcher in the field: be precise and technical, name how it departs from prior methods, give the key evidence with its numbers, and state assumptions and limits.",
    }
}

pub const EXPLAIN: &str = "You explain an arXiv paper from its title and abstract to one reader, at their level. \
Answer with JSON only: {\"explain\": \"4 to 6 sentences: the problem, why it matters, the idea, and what was found, with a number if the abstract gives one\", \
\"terms\": [{\"term\": \"a hard term from the abstract, as written there\", \"meaning\": \"at most 20 words, at the reader's level\"}]}. \
List the terms this reader is least likely to know, each a name of a method, model, concept or measure used in the abstract: 3 to 5 for a student, 2 to 4 for a practitioner, and for an expert 0 to 3, only ones new in this paper or particular to a narrow subfield. \
Use only what the abstract says; no hype, no preamble.";

/// The paper the field credits with a term, as Who coined it reads it:
/// among the 30 most cited papers that use it as a phrase, the earliest
/// with a quarter of the top count (and 20) or more. None when the words
/// were in use before arXiv began, so its first papers here would mislead.
pub async fn coined(c: &Ctx<'_>, term: &str) -> Option<Value> {
    let term = term.replace('"', "");
    let words = term.split_whitespace().count();
    if words == 0 || words > 6 {
        return None;
    }
    let p = json!({"p": format!("\"{term}\"")});
    let (rows, early) = tokio::join!(
        c.soft("its most cited papers", "SELECT id, title, year, cites, created FROM ax_papers WHERE (MATCH(title, :p) OR MATCH(abstract, :p)) ORDER BY cites DESC LIMIT 30", p.clone()),
        c.soft("its uses before 1994", "SELECT COUNT(*) AS n FROM ax_papers WHERE (MATCH(title, :p) OR MATCH(abstract, :p)) AND year <= 1993", p),
    );
    if early.first().map(|r| r.i("n")).unwrap_or(0) >= 5 {
        return None;
    }
    let top = rows.first().map(|r| r.i("cites")).unwrap_or(0);
    let mut credited: Vec<&Row> = rows.iter().filter(|r| r.i("cites") * 4 >= top && r.i("cites") >= 20).collect();
    credited.sort_by_key(|r| r.s("created"));
    credited.first().map(|r| json!({"id": r.s("id"), "title": r.s("title"), "year": r.i("year"), "cites": r.i("cites"), "created": r.s("created")}))
}

/// A paper explained for a reader at a level, its hard terms each with the
/// paper that brought it in; kept or written now.
pub async fn explain(c: &Ctx<'_>, id: &str, level: &str, model: &str) -> Result<Value, String> {
    let id = understand::clean_id(id);
    let k = format!("explain:{level}:{id}");
    let kept = c.gsoft("a kept explanation", "SELECT text, model, at FROM ax_notes WHERE id = :k", json!({"k": k})).await;
    if let Some(r) = kept.first().filter(|r| !r.s("text").trim().is_empty()) {
        let v: Value = serde_json::from_str(&r.s("text")).unwrap_or(Value::Null);
        return Ok(json!({"id": id, "level": level, "note": v, "model": r.s("model"), "kept": true}));
    }
    let row = c.soft("the paper", "SELECT id, title, abstract, created FROM ax_papers WHERE id = :id", json!({"id": id})).await.into_iter().next().ok_or("no such paper")?;
    let t0 = std::time::Instant::now();
    let text = llm(c, "the explanation (llm() on the server)", model, EXPLAIN, &format!("{}\nTitle: {}", level_note(level), row.s("title")), &format!("Abstract: {}", row.s("abstract")), 1500).await?;
    let mut v = json_in(&text).unwrap_or(json!({"explain": text.trim(), "terms": []}));
    // Each term with the paper that brought it in, when that paper came before this one.
    let terms: Vec<Value> = v["terms"].as_array().cloned().unwrap_or_default().into_iter().take(5).collect();
    let found = futures_util::future::join_all(terms.iter().map(|t| coined(c, t["term"].as_str().unwrap_or("")))).await;
    let created = row.s("created");
    let terms: Vec<Value> = terms
        .into_iter()
        .zip(found)
        .map(|(mut t, f)| {
            // Credited to a later paper is no origin; to this one, it is said so.
            if let Some(mut f) = f.filter(|f| f["created"].as_str().unwrap_or("~") <= created.as_str()) {
                f["here"] = json!(f["id"] == json!(id));
                t["from"] = f;
            }
            t
        })
        .collect();
    v["terms"] = json!(terms);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": format!("explain-{level}"), "text": v.to_string(), "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": ms})]).await;
    Ok(json!({"id": id, "level": level, "note": v, "model": model, "kept": false, "ms": ms}))
}

/// The three earlier papers closest to one: the nearest by meaning that
/// were posted before it, the ones it cites first.
pub async fn earlier(c: &Ctx<'_>, id: &str) -> (Option<Row>, Vec<Row>) {
    let p = json!({"id": id});
    let (row, refs) = tokio::join!(
        c.soft("the paper", "SELECT id, title, abstract, created, year FROM ax_papers WHERE id = :id", p.clone()),
        c.gsoft("what it cites (on arXiv)", "SELECT dst FROM ax_cites WHERE src = :id LIMIT 10000", p.clone()),
    );
    let Some(row) = row.into_iter().next() else { return (None, Vec::new()) };
    let near = c.soft("the closest earlier papers by meaning", "SELECT id, title, abstract, authors, nauthors, year, cites, created, score() AS s FROM ax_papers WHERE created < :c AND id != :id ORDER BY similarity(emb, :v) DESC LIMIT 40", json!({"id": id, "c": row.s("created"), "v": {"vector_of": id, "table": "ax_papers", "field": "emb"}})).await;
    // Among the 40 closest, the much cited first: Llama 2 is set against
    // LLaMA rather than a small paper that tuned LLaMA for one task.
    let mut near = near;
    let weight = |r: &Row| r.f("s") + 0.03 * (1.0 + r.f("cites").max(0.0)).ln();
    near.sort_by(|a, b| weight(b).partial_cmp(&weight(a)).unwrap_or(std::cmp::Ordering::Equal));
    let cites: std::collections::HashSet<String> = refs.iter().map(|r| r.s("dst")).collect();
    let title = row.s("title").to_lowercase();
    let mut picked: Vec<Row> = near.iter().filter(|r| cites.contains(&r.s("id"))).take(2).cloned().collect();
    for r in near.iter() {
        if picked.len() >= 3 {
            break;
        }
        // Not a copy of it under another id.
        if r.s("title").to_lowercase() != title && !picked.iter().any(|x| x.s("id") == r.s("id")) {
            picked.push(r.clone());
        }
    }
    for r in picked.iter_mut() {
        let k = r.s("id");
        r.insert("cited".into(), json!(cites.contains(&k)));
    }
    (Some(row), picked)
}

/// What a paper adds to the earlier papers closest to it, kept or written now.
pub async fn novelty(c: &Ctx<'_>, id: &str, model: &str) -> Result<Value, String> {
    let id = understand::clean_id(id);
    let k = format!("new:{id}");
    let (row, picked) = earlier(c, &id).await;
    let row = row.ok_or("no such paper")?;
    let shown: Vec<Value> = picked.iter().enumerate().map(|(i, r)| json!({"n": i + 1, "id": r.s("id"), "title": r.s("title"), "authors": crate::answer::first_author(r), "year": r.i("year"), "cites": r.i("cites"), "cited": r.get("cited"), "close": (r.f("s") * 100.0).round() / 100.0})).collect();
    let kept = c.gsoft("a kept comparison", "SELECT text, model, at FROM ax_notes WHERE id = :k", json!({"k": k})).await;
    if let Some(r) = kept.first().filter(|r| !r.s("text").trim().is_empty()) {
        let v: Value = serde_json::from_str(&r.s("text")).unwrap_or(json!({"adds": r.s("text")}));
        return Ok(json!({"id": id, "note": v["note"], "earlier": v["earlier"], "model": r.s("model"), "kept": true}));
    }
    if picked.is_empty() {
        return Ok(json!({"id": id, "note": {"adds": "Nothing earlier on arXiv is close to it.", "kind": "new problem"}, "earlier": [], "kept": false}));
    }
    let t0 = std::time::Instant::now();
    let ctx = format!("Earlier papers:\n\n{}", context(&picked));
    let text = llm(c, "what it adds (llm() on the server)", model, NOVELTY, &format!("The paper: “{}” ({})\nAbstract: {}", row.s("title"), row.i("year"), row.s("abstract")), &ctx, 1200).await?;
    let note = json_in(&text).unwrap_or(json!({"adds": text.trim()}));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let keep = json!({"note": note, "earlier": shown});
    let _ = c.g.upsert("ax_notes", vec![json!({"id": k, "paper": id, "style": "new", "text": keep.to_string(), "model": model, "at": chrono::Utc::now().to_rfc3339(), "ms": ms})]).await;
    Ok(json!({"id": id, "note": note, "earlier": shown, "model": model, "kept": false, "ms": ms}))
}

/// The first JSON object in a model's answer.
pub fn json_in(text: &str) -> Option<Value> {
    let a = text.find('{')?;
    let b = text.rfind('}')?;
    serde_json::from_str(text.get(a..=b)?).ok()
}
