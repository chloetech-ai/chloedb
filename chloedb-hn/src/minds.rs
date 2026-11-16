//! Changed minds: one person's take on a topic over the years.
//!
//! The people are those who wrote about the topic in the most years: one
//! `GROUP BY author` over the comments whose words match it, then their
//! comments counted by year. A person's arc is a few of their comments on
//! the topic a year (the ones BM25 finds most about it), each read by a
//! model through the server's `llm()` for where it stands, from -2 to 2,
//! with a phrase copied from the comment that shows it; a phrase the
//! comment does not hold is dropped, so every quote shown is the person's
//! own words. An arc is made once and kept in `hn_answers`.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::search::Ctx;
use crate::strata::{Row, RowExt};

/// Comments a year an arc reads, and in all.
const PER_YEAR: usize = 3;
const MOST: usize = 45;

/// Topics the page offers before one is typed.
pub const TOPICS: &[&str] = &["rust", "bitcoin", "kubernetes", "remote work", "llm", "electron", "microservices", "nuclear power", "tesla", "typescript", "go", "graphql"];

/// A topic as the arcs are kept under: its words, lowercase, one space apart.
pub fn norm(topic: &str) -> String {
    topic.split_whitespace().map(|w| w.to_lowercase()).collect::<Vec<_>>().join(" ")
}

pub fn key(topic: &str, author: &str) -> String {
    format!("mind:{}:{author}", norm(topic))
}

/// The words of a topic for `MATCH`: a phrase when it has more than one.
fn words(topic: &str) -> String {
    let t = norm(topic).replace('"', "");
    if t.contains(' ') { format!("\"{t}\"") } else { t }
}

/// The people who wrote about a topic in the most years, with their
/// comments on it a year, and the arcs already made.
pub async fn people(c: &Ctx<'_>, topic: &str) -> Value {
    let t = words(topic);
    if t.is_empty() {
        return json!({"topic": "", "people": [], "topics": TOPICS});
    }
    let top = c
        .soft(
            "who wrote about it most",
            "SELECT author, COUNT(*) AS n, MIN(year) AS y0, MAX(year) AS y1 FROM hn_comments WHERE MATCH(body, :t, synonyms => 1) AND author <> '' GROUP BY author ORDER BY n DESC LIMIT 80",
            json!({"t": t}),
        )
        .await;
    let total = c.soft("comments about it", "SELECT COUNT(*) AS n FROM hn_comments WHERE MATCH(body, :t, synonyms => 1)", json!({"t": t})).await.first().map(|r| r.i("n")).unwrap_or(0);
    // Four years apart at least: a mind has to have had time to change.
    let picked: Vec<&Row> = top.iter().filter(|r| r.i("y1") - r.i("y0") >= 4 && r.i("n") >= 12).take(40).collect();
    if picked.is_empty() {
        return json!({"topic": norm(topic), "people": [], "comments": total, "topics": TOPICS});
    }
    let names: Vec<String> = picked.iter().map(|r| format!("'{}'", r.s("author").replace('\'', "''"))).collect();
    let keys: Vec<String> = picked.iter().map(|r| format!("'{}'", key(topic, &r.s("author")).replace('\'', "''"))).collect();
    let (by_year, kept) = tokio::join!(
        c.soft("their comments on it, a year", &format!("SELECT author, year, COUNT(*) AS n FROM hn_comments WHERE MATCH(body, :t, synonyms => 1) AND author IN ({}) GROUP BY author, year", names.join(", ")), json!({"t": t})),
        c.soft("the arcs made before", &format!("SELECT id, answer FROM hn_answers WHERE id IN ({})", keys.join(", ")), Value::Null),
    );
    let mut years: BTreeMap<String, BTreeMap<i64, i64>> = BTreeMap::new();
    for r in &by_year {
        years.entry(r.s("author")).or_default().insert(r.i("year"), r.i("n"));
    }
    let arcs: std::collections::HashMap<String, Value> = kept.iter().filter_map(|r| Some((r.s("id"), serde_json::from_str::<Value>(&r.s("answer")).ok()?))).collect();
    let mut people: Vec<Value> = picked
        .iter()
        .map(|r| {
            let a = r.s("author");
            let ys = years.get(&a).cloned().unwrap_or_default();
            let arc = arcs.get(&key(topic, &a)).map(|v| json!({"changed": v["changed"], "from": v["from"], "to": v["to"], "shift": v["shift"], "years": v["years"]}));
            json!({"author": a, "n": r.i("n"), "first": r.i("y0"), "last": r.i("y1"), "years": ys.iter().map(|(y, n)| json!({"year": y, "n": n})).collect::<Vec<_>>(), "active_years": ys.len(), "arc": arc})
        })
        .collect();
    // The arcs made, the biggest change of mind first; then the rest by
    // the years they wrote about it in.
    people.sort_by(|a, b| {
        let s = |v: &Value| v["arc"]["shift"].as_f64().map(f64::abs);
        match (s(a), s(b)) {
            (Some(x), Some(y)) => y.total_cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b["active_years"].as_i64().cmp(&a["active_years"].as_i64()).then(b["n"].as_i64().cmp(&a["n"].as_i64())),
        }
    });
    json!({"topic": norm(topic), "people": people, "comments": total, "topics": TOPICS})
}

/// The comments an arc reads: the most on the topic of each year, oldest
/// first.
fn pick(rows: Vec<Row>) -> Vec<Row> {
    let mut by: BTreeMap<i64, Vec<Row>> = BTreeMap::new();
    for r in rows {
        if r.s("body").chars().count() >= 120 {
            by.entry(r.i("year")).or_default().push(r);
        }
    }
    let mut out: Vec<Row> = by.into_values().flat_map(|v| v.into_iter().take(PER_YEAR)).collect();
    // At most MOST: the years thinned evenly.
    if out.len() > MOST {
        let step = out.len() as f64 / MOST as f64;
        out = (0..MOST).map(|i| out[(i as f64 * step) as usize].clone()).collect();
    }
    out.sort_by(|a, b| a.s("created").cmp(&b.s("created")));
    out
}

fn instructions(topic: &str) -> String {
    format!(
        "You read one Hacker News member's comments about \"{topic}\", oldest first, to see whether their view of it changed over the years.\n\
For each comment give: its id; stance, a whole number from -2 to 2 for how the writer feels about {topic} itself (-2 strongly against or negative, -1 critical, 0 neutral, mixed or not really about it, 1 favourable, 2 strongly for); and quote, a short phrase of at most 20 words copied exactly from that comment that shows the stance.\n\
Then: changed (true only if their overall view clearly moved), from and to (their view early and late, a few words each, e.g. \"skeptical of the borrow checker\" and \"ships Rust at work\"), and summary (two sentences on how their view went, in plain words, no praise).\n\
Answer with JSON only: {{\"comments\":[{{\"id\":1,\"stance\":0,\"quote\":\"...\"}}],\"changed\":false,\"from\":\"...\",\"to\":\"...\",\"summary\":\"...\"}}"
    )
}

/// Letters and digits, lowercase, one space apart: how a quote is looked
/// for in its comment.
fn plain(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_json(s: &str) -> Option<Value> {
    let a = s.find('{')?;
    let b = s.rfind('}')?;
    serde_json::from_str(&s[a..=b]).ok()
}

/// The model's reading of the comments, checked: a stance of each comment
/// read, the quote kept only if the comment holds it, the mean stance of
/// each year, and the shift, the last years' mean less the first years'.
pub fn arc_of(read: &Value, comments: &[Row]) -> Value {
    let by_id: std::collections::HashMap<i64, &Row> = comments.iter().map(|r| (r.i("id"), r)).collect();
    let mut points = Vec::new();
    for x in read["comments"].as_array().into_iter().flatten() {
        let Some(id) = x["id"].as_i64() else { continue };
        let Some(r) = by_id.get(&id) else { continue };
        let stance = x["stance"].as_i64().unwrap_or(0).clamp(-2, 2);
        let quote = x["quote"].as_str().unwrap_or("").trim().to_string();
        let held = !quote.is_empty() && plain(&r.s("body")).contains(&plain(&quote));
        points.push(json!({"id": id, "year": r.i("year"), "created": r.s("created"), "stance": stance, "quote": if held { json!(quote) } else { Value::Null }, "story": r.i("story"), "story_title": r.s("story_title")}));
    }
    let mut years: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
    for p in &points {
        let e = years.entry(p["year"].as_i64().unwrap_or(0)).or_default();
        e.0 += p["stance"].as_f64().unwrap_or(0.0);
        e.1 += 1.0;
    }
    let ys: Vec<(i64, f64)> = years.iter().map(|(y, (s, n))| (*y, s / n)).collect();
    // The first third of the years against the last third.
    let k = (ys.len() / 3).max(1);
    let mean = |v: &[(i64, f64)]| if v.is_empty() { 0.0 } else { v.iter().map(|x| x.1).sum::<f64>() / v.len() as f64 };
    let shift = if ys.len() >= 2 { mean(&ys[ys.len() - k..]) - mean(&ys[..k]) } else { 0.0 };
    json!({
        "points": points,
        "years": ys.iter().map(|(y, s)| json!({"year": y, "stance": (s * 100.0).round() / 100.0})).collect::<Vec<_>>(),
        "shift": (shift * 100.0).round() / 100.0,
        "changed": read["changed"].as_bool().unwrap_or(false),
        "from": read["from"], "to": read["to"], "summary": read["summary"],
    })
}

/// A person's arc on a topic, kept or made now (`made` says which).
pub async fn arc(c: &Ctx<'_>, topic: &str, author: &str, model: &str) -> Result<Value, String> {
    let k = key(topic, author);
    if let Some(r) = c.soft("a kept arc", "SELECT answer, model, at FROM hn_answers WHERE id = :k", json!({"k": k})).await.first() {
        if let Some(mut v) = parse_json(&r.s("answer")) {
            v["kept"] = json!(true);
            v["model"] = json!(r.s("model"));
            v["at"] = json!(r.s("at"));
            return Ok(v);
        }
    }
    make(c, topic, author, model).await
}

/// Makes a person's arc on a topic, and keeps it.
pub async fn make(c: &Ctx<'_>, topic: &str, author: &str, model: &str) -> Result<Value, String> {
    let t = words(topic);
    let rows = c
        .rows(
            "their comments on it, the most about it first",
            "SELECT id, year, created, body, story, story_title FROM hn_comments WHERE author = :a AND MATCH(body, :t, synonyms => 1) ORDER BY bm25(body, :t, synonyms => 1) DESC LIMIT 400",
            json!({"a": author, "t": t}),
        )
        .await
        .map_err(|e| e.to_string())?;
    let picked = pick(rows);
    if picked.len() < 4 {
        return Err(format!("{author} wrote too little about {} to read a change of mind", norm(topic)));
    }
    let ctx = picked.iter().map(|r| format!("[{}] {}: {}", r.i("id"), r.s("created").chars().take(7).collect::<String>(), crate::text::clip(&r.s("body"), 700))).collect::<Vec<_>>().join("\n\n");
    let t0 = std::time::Instant::now();
    // The model thinks before it answers, inside max_tokens: room for both.
    let sql = format!("SELECT id, llm('{}', :instructions, :question, :context, max_tokens => 7000) AS answer FROM hn_stories LIMIT 1", model.replace('\'', "''"));
    let got = c
        .rows("the reading (llm() on the server)", &sql, json!({"instructions": instructions(&norm(topic)), "question": format!("{author} on {}: {} comments", norm(topic), picked.len()), "context": ctx}))
        .await
        .map_err(|e| e.to_string())?;
    let read = got.first().and_then(|r| parse_json(&r.s("answer"))).ok_or("the model's answer was not the reading asked for: try again later")?;
    let mut v = arc_of(&read, &picked);
    v["author"] = json!(author);
    v["topic"] = json!(norm(topic));
    v["read"] = json!(picked.len());
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let ids: Vec<i64> = picked.iter().map(|r| r.i("id")).collect();
    let _ = c.db.write(vec![json!({"op": "upsert", "table": "hn_answers", "doc": {"id": key(topic, author), "kind": "mind", "question": format!("{author} on {}", norm(topic)), "answer": v.to_string(), "sources": json!(ids).to_string(), "model": model, "at": at, "ms": ms}})]).await;
    v["kept"] = json!(false);
    v["model"] = json!(model);
    v["at"] = json!(at);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, year: i64, body: &str) -> Row {
        json!({"id": id, "year": year, "created": format!("{year}-03-01T00:00:00Z"), "body": body, "story": 1, "story_title": "t"}).as_object().unwrap().clone()
    }

    #[test]
    fn quotes_are_the_persons_own_words() {
        let rows = vec![row(1, 2014, "I drool over Rust, too. It is *amazing*."), row(2, 2024, "Rust is far too similar to C++ for my taste.")];
        let read = json!({"comments": [
            {"id": 1, "stance": 2, "quote": "I drool over Rust, too"},
            {"id": 2, "stance": -1, "quote": "Rust is a disaster"},
            {"id": 9, "stance": 1, "quote": "not one of the comments"},
        ], "changed": true, "from": "loves it", "to": "finds it too much like C++", "summary": "s"});
        let v = arc_of(&read, &rows);
        let p = v["points"].as_array().unwrap();
        assert_eq!(p.len(), 2, "a comment that was not read is dropped");
        assert_eq!(p[0]["quote"], "I drool over Rust, too");
        assert!(p[1]["quote"].is_null(), "a quote the comment does not hold is dropped");
        assert_eq!(v["shift"], -3.0);
        assert_eq!(v["changed"], true);
    }

    #[test]
    fn a_few_comments_a_year() {
        let long = "x".repeat(130);
        let rows: Vec<Row> = (0..10).flat_map(|y| (0..5).map(move |i| (y, i))).map(|(y, i)| row(y * 10 + i, 2010 + y, &long)).collect();
        let p = pick(rows);
        assert_eq!(p.len(), 30, "three a year");
        assert!(p.windows(2).all(|w| w[0].s("created") <= w[1].s("created")), "oldest first");
        assert!(pick(vec![row(1, 2010, "short")]).is_empty(), "too short to say anything");
        assert_eq!(key("Remote  Work", "pg"), "mind:remote work:pg");
        assert_eq!(words("remote work"), "\"remote work\"");
    }
}
