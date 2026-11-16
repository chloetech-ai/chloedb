//! A discussion: the story and every comment under it, as one read of the
//! comments' table (clustered by story, so a thread's rows sit together),
//! with the comment that was found marked and the way to it opened.
//! Beside it: the same link's other discussions, the discussions this one
//! links to and those that link here (an edge table), and the nearest
//! stories by meaning.

use serde_json::{json, Value};

use crate::search::{Ctx, STORY_COLS};
use crate::strata::{Row, RowExt};

pub async fn story_row(c: &Ctx<'_>, id: i64) -> Option<Row> {
    c.soft("the story", &format!("SELECT {STORY_COLS}, body, curl FROM hn_stories WHERE id = {id}"), Value::Null).await.into_iter().next()
}

pub async fn comment_row(c: &Ctx<'_>, id: i64) -> Option<Row> {
    c.soft("the comment", &format!("SELECT id, story, parent, author FROM hn_comments WHERE id = {id}"), Value::Null).await.into_iter().next()
}

/// The thread an item is in, `focus` the item asked for.
pub async fn thread(c: &Ctx<'_>, id: i64) -> Option<Value> {
    let (story_id, focus) = match story_row(c, id).await {
        Some(_) => (id, None),
        None => {
            let cm = comment_row(c, id).await?;
            (cm.i("story"), Some(id))
        }
    };
    let story = story_row(c, story_id).await;
    let comments = c.soft("the thread's comments", &format!("SELECT id, body, author, created, parent, depth, pos, replies, thread FROM hn_comments WHERE story = {story_id} ORDER BY id LIMIT 10000"), Value::Null).await;
    // The way from the story down to the comment asked for.
    let mut path = Vec::new();
    if let Some(f) = focus {
        let by_id: std::collections::HashMap<i64, i64> = comments.iter().map(|r| (r.i("id"), r.i("parent"))).collect();
        let mut at = f;
        for _ in 0..200 {
            path.push(at);
            match by_id.get(&at) {
                Some(&p) if p != story_id && p != 0 => at = p,
                _ => break,
            }
        }
        path.reverse();
    }
    Some(json!({"story": story, "story_id": story_id, "comments": comments, "focus": focus, "path": path, "loaded": story.is_some()}))
}

/// What is around a story: the same link discussed again, links between
/// discussions, and the nearest stories by meaning.
pub async fn around(c: &Ctx<'_>, id: i64) -> Value {
    let Some(s) = story_row(c, id).await else { return json!({}) };
    let curl = s.s("curl");
    let same = if curl.is_empty() {
        Vec::new()
    } else {
        c.soft("the same link, discussed again", &format!("SELECT {STORY_COLS} FROM hn_stories WHERE curl = :curl AND id <> {id} ORDER BY created DESC LIMIT 20"), json!({"curl": curl})).await
    };
    let (out_links, in_links, similar) = tokio::join!(
        c.soft("discussions this one links to", &format!("SELECT {STORY_COLS} FROM hn_stories WHERE REACHABLE(hn_links, {id}, dir => 'out') ORDER BY points DESC LIMIT 12"), Value::Null),
        c.soft("discussions that link here", &format!("SELECT {STORY_COLS} FROM hn_stories WHERE REACHABLE(hn_links, {id}, dir => 'in') ORDER BY points DESC LIMIT 12"), Value::Null),
        c.soft("the nearest stories by meaning", &format!("SELECT {STORY_COLS}, curl, score() AS score FROM hn_stories WHERE id <> {id} ORDER BY similarity(emb, :v) * (1 + 0.15 * pop) DESC LIMIT 12"), json!({"v": {"vector_of": id, "table": "hn_stories", "field": "emb"}})),
    );
    let site = s.s("site");
    let site_stats = if site.is_empty() {
        Value::Null
    } else {
        let r = c.soft("the site on HN", "SELECT COUNT(*) AS n, SUM(points) AS points, MIN(created) AS first FROM hn_stories WHERE site = :site", json!({"site": site})).await;
        json!({"site": site, "stories": r.first().map(|x| x.i("n")), "points": r.first().map(|x| x.i("points")), "first": r.first().map(|x| x.s("first"))})
    };
    let similar: Vec<Row> = similar.into_iter().filter(|r| r.s("curl") != curl || curl.is_empty()).collect();
    json!({"same_link": same, "links_to": out_links, "linked_from": in_links, "similar": similar, "site": site_stats})
}
