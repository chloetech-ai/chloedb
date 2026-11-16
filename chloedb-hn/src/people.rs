//! People: a person's page, who knows a topic, and the reply graph between
//! them. The graph is an edge table (`hn_replies`, from the one who replied
//! to the one answered, weighted by how often), so "who is in someone's
//! orbit" is a personalized PageRank (`ppr`) from them, and "the community
//! around a topic" is one seeded by the people who wrote its best comments.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::query::{self, Parsed};
use crate::search::{Ctx, STORY_COLS};
use crate::strata::{Row, RowExt};

pub async fn person(c: &Ctx<'_>, user: &str) -> Value {
    let p = json!({"u": user});
    let row = c.soft("the person", "SELECT id, stories, comments, points, first, last, replies_got, talked_to, about, karma FROM hn_users WHERE id = :u", p.clone()).await.into_iter().next();
    let (top, sites, topics, years, replies_to, replied_by, orbit, recent) = tokio::join!(
        c.soft("their best stories", &format!("SELECT {STORY_COLS} FROM hn_stories WHERE author = :u ORDER BY points DESC LIMIT 10"), p.clone()),
        c.soft("the sites they post", "SELECT site, COUNT(*) AS n, SUM(points) AS points FROM hn_stories WHERE author = :u AND site <> '' GROUP BY site ORDER BY n DESC LIMIT 8", p.clone()),
        c.soft("what they post about", "SELECT topics, COUNT(*) AS n FROM hn_stories WHERE author = :u GROUP BY topics ORDER BY n DESC LIMIT 10", p.clone()),
        c.soft("comments a year", "SELECT year, COUNT(*) AS n FROM hn_comments WHERE author = :u GROUP BY year ORDER BY year", p.clone()),
        c.soft("whom they answer most", "SELECT dst AS who, w FROM hn_replies WHERE src = :u ORDER BY w DESC LIMIT 10", p.clone()),
        c.soft("who answers them most", "SELECT src AS who, w FROM hn_replies WHERE dst = :u ORDER BY w DESC LIMIT 10", p.clone()),
        c.soft("their orbit (PageRank from them over the reply graph)", "SELECT id, comments, points, score() AS score FROM hn_users WHERE id <> :u ORDER BY ppr(hn_replies, :u) DESC LIMIT 12", p.clone()),
        c.soft("their latest comments", "SELECT id, body, created, story, story_title, replies, thread FROM hn_comments WHERE author = :u ORDER BY created DESC LIMIT 8", p.clone()),
    );
    json!({"user": row, "id": user, "top": top, "sites": sites, "topics": topics, "years": years, "replies_to": replies_to, "replied_by": replied_by, "orbit": orbit, "recent": recent})
}

/// Who knows a topic: the people whose comments on it rank best, by how
/// many such comments, how good (BM25 and the discussion each started),
/// over how many threads; then the community around them by the reply
/// graph, and the edges among the top people for a picture.
pub async fn experts(c: &Ctx<'_>, p: &Parsed) -> Value {
    let words = query::plain_words(p);
    if words.is_empty() {
        return json!({"experts": [], "edges": []});
    }
    let params = json!({"q": words, "m": query::match_text(p)});
    let rows = c.soft("the best comments on the topic", "SELECT id, author, story, story_title, thread, pos, score() AS score FROM hn_comments WHERE MATCH(body, :m, synonyms => 1) ORDER BY bm25(body, :q, synonyms => 1) * (1 + 0.3 * pop) DESC LIMIT 3000", params.clone()).await;
    #[derive(Default)]
    struct E {
        score: f64,
        n: i64,
        threads: std::collections::HashSet<i64>,
        best: Vec<(f64, Row)>,
    }
    let mut by: HashMap<String, E> = HashMap::new();
    let top_score = rows.first().map(|r| r.f("score")).unwrap_or(1.0).max(1e-6);
    for r in &rows {
        let a = r.s("author");
        if a.is_empty() {
            continue;
        }
        let e = by.entry(a).or_default();
        let s = r.f("score") / top_score;
        e.score += s;
        e.n += 1;
        e.threads.insert(r.i("story"));
        if e.best.len() < 3 {
            e.best.push((s, r.clone()));
        }
    }
    // Breadth matters: many threads beat one long argument.
    let mut list: Vec<(String, f64, E)> = by.into_iter().map(|(a, e)| {
        let breadth = (e.threads.len() as f64).sqrt();
        let s = e.score * breadth / (e.n as f64).sqrt().max(1.0) * (1.0 + (e.n as f64).ln());
        (a, s, e)
    }).collect();
    list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    list.truncate(24);
    let names: Vec<String> = list.iter().map(|x| x.0.clone()).collect();
    let quoted: Vec<String> = names.iter().map(|n| format!("'{}'", n.replace('\'', "''"))).collect();
    let edges = if quoted.len() > 1 {
        c.soft("who among them replies to whom", &format!("SELECT src, dst, w FROM hn_replies WHERE src IN ({0}) AND dst IN ({0}) ORDER BY w DESC LIMIT 200", quoted.join(", ")), Value::Null).await
    } else {
        Vec::new()
    };
    let community = if quoted.len() > 2 {
        c.soft("the community around them (PageRank from the top people)", &format!("SELECT id, comments, score() AS score FROM hn_users WHERE id NOT IN ({0}) ORDER BY ppr(hn_replies, ({0})) DESC LIMIT 12", quoted.join(", ")), Value::Null).await
    } else {
        Vec::new()
    };
    let users = if quoted.is_empty() { Vec::new() } else {
        c.soft("their profiles", &format!("SELECT id, comments, stories, points, first FROM hn_users WHERE id IN ({})", quoted.join(", ")), Value::Null).await
    };
    let prof: HashMap<String, Row> = users.into_iter().map(|r| (r.s("id"), r)).collect();
    let experts: Vec<Value> = list.iter().map(|(a, s, e)| json!({
        "id": a, "score": (s * 100.0).round() / 100.0, "comments_on_topic": e.n, "threads": e.threads.len(),
        "profile": prof.get(a), "best": e.best.iter().map(|(_, r)| json!({"id": r.i("id"), "story": r.i("story"), "story_title": r.s("story_title"), "thread": r.i("thread")})).collect::<Vec<_>>(),
    })).collect();
    json!({"experts": experts, "edges": edges, "community": community, "read": rows.len()})
}
