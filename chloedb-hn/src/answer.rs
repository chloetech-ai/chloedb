//! The answer to a question asked in a sentence ([`crate::nl`]), above the
//! results: the one story it names, a count, people, sites or years. Each
//! is the result of ordinary SQL (shown with the answer), no model.

use serde_json::{json, Value};
use tokio::sync::OnceCell;

use crate::nl::Ask;
use crate::query::{self, Parsed};
use crate::search::{self, Ctx};
use crate::strata::{Row, RowExt};

/// Stories a year, over the whole archive: what a year's share is of.
static YEARS: OnceCell<Vec<(i64, i64)>> = OnceCell::const_new();

fn n(x: i64) -> String {
    let s = x.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if x < 0 { format!("-{out}") } else { out }
}

/// The question's scope in words: "about rust, Show HN, in 2023".
fn scope(p: &Parsed) -> String {
    let mut parts: Vec<String> = Vec::new();
    let words = query::plain_words(p);
    if !words.is_empty() {
        parts.push(format!("about “{words}”"));
    }
    for note in p.notes.iter().skip(if p.ask.is_some() { 1 } else { 0 }) {
        parts.push(note.what.clone());
    }
    if parts.is_empty() { String::new() } else { format!(" {}", parts.join(", ")) }
}

/// The filters and the words of a question as a `WHERE`, over stories or
/// comments.
fn wh(p: &Parsed, stories: bool, params: &mut serde_json::Map<String, Value>) -> String {
    let mut conds = search::filters(p, stories, params);
    conds.extend(search::excluded(p, if stories { "title" } else { "body" }, params));
    let words = query::plain_words(p);
    if !words.is_empty() {
        params.insert("m".into(), json!(query::match_text(p)));
        params.insert("min".into(), json!(search::min_words(&words)));
        conds.push(if stories { search::STORY_MATCH } else { search::COMMENT_MATCH }.to_string());
    }
    if conds.is_empty() { String::new() } else { format!("WHERE {}", conds.join(" AND ")) }
}

/// The answer, when the search was a question.
pub async fn answer(c: &Ctx<'_>, p: &Parsed, stories: &[Row], comments: &[Row], total_s: i64, total_c: i64, facets: Option<&Value>) -> Option<Value> {
    let ask = p.ask.clone()?;
    let sc = scope(p);
    let on_comments = p.only.as_deref() == Some("comment");
    match ask {
        Ask::Top { by, one } => {
            let (row, kind) = if on_comments { (comments.first()?, "comment") } else { (stories.first()?, "story") };
            let what = if on_comments { "comment" } else { "story" };
            let lead = match by.as_str() {
                "comments" if on_comments => format!("The {what} with the most replies"),
                "comments" => format!("The {what} with the most comments"),
                "points" if on_comments => "HN does not publish comments' points; the comment that started the most discussion".to_string(),
                "points" => format!("The {what} with the most points"),
                "date" => format!("The newest {what}"),
                "old" => format!("The first {what}"),
                "least_points" => format!("The {what} with the fewest points"),
                "least_comments" => format!("The {what} with the fewest comments"),
                _ => format!("The top {what}"),
            };
            let shown = if one { 1 } else { p.limit.unwrap_or(3).min(10) };
            let list: Vec<Value> = if on_comments { comments.iter().take(shown).map(|r| json!(r)).collect() } else { stories.iter().take(shown).map(|r| json!(r)).collect() };
            Some(json!({"kind": "top", "by": by, "one": one, "title": format!("{lead}{sc}"), "item_kind": kind, "item": row, "items": list}))
        }
        Ask::Count => {
            let (total, what) = if on_comments { (total_c, if total_c == 1 { "comment" } else { "comments" }) } else { (total_s, if total_s == 1 { "story" } else { "stories" }) };
            let words = query::plain_words(p);
            let verb = if words.is_empty() { String::new() } else { format!(" mention “{words}”") };
            let rest: Vec<String> = p.notes.iter().skip(1).map(|x| x.what.clone()).collect();
            let rest = if rest.is_empty() { String::new() } else { format!(" ({})", rest.join(", ")) };
            let years = facets.and_then(|f| f.get("year")).cloned().unwrap_or(Value::Null);
            Some(json!({"kind": "count", "n": total, "what": what, "title": format!("{} {what}{verb}{rest}", n(total)), "years": years}))
        }
        Ask::People { by } => {
            let mut params = serde_json::Map::new();
            let (sql, what, note) = match by.as_str() {
                "comments" => {
                    let w = wh(p, false, &mut params);
                    (format!("SELECT author, COUNT(*) AS n FROM hn_comments {} author != '' GROUP BY author ORDER BY n DESC LIMIT 10", if w.is_empty() { "WHERE".to_string() } else { format!("{w} AND") }), "comments", "")
                }
                "karma" | "points" => {
                    let w = wh(p, true, &mut params);
                    let note = if by == "karma" { "HN's archive does not carry karma: ranked by the points their stories earned" } else { "" };
                    (format!("SELECT author, SUM(points) AS n, COUNT(*) AS stories FROM hn_stories {} author != '' GROUP BY author ORDER BY n DESC LIMIT 10", if w.is_empty() { "WHERE".to_string() } else { format!("{w} AND") }), "points", note)
                }
                "knows" => {
                    let e = crate::people::experts(c, p).await;
                    let list: Vec<Value> = e["experts"].as_array().cloned().unwrap_or_default().into_iter().take(10).map(|x| json!({"author": x["id"], "n": x["comments_on_topic"], "threads": x["threads"]})).collect();
                    return Some(json!({"kind": "people", "by": by, "title": format!("Who knows the most{sc}"), "what": "comments on it", "people": list}));
                }
                _ => {
                    let w = wh(p, true, &mut params);
                    (format!("SELECT author, COUNT(*) AS n, SUM(points) AS points FROM hn_stories {} author != '' GROUP BY author ORDER BY n DESC LIMIT 10", if w.is_empty() { "WHERE".to_string() } else { format!("{w} AND") }), "stories", "")
                }
            };
            let rows = c.soft("the people who answer it", &sql, Value::Object(params)).await;
            let lead = match by.as_str() {
                "comments" => "Who commented the most",
                "karma" => "Who has earned the most points",
                "points" => "Who earned the most points",
                _ => "Who posted the most stories",
            };
            Some(json!({"kind": "people", "by": by, "title": format!("{lead}{sc}"), "what": what, "note": note, "people": rows}))
        }
        Ask::Sites => {
            let mut params = serde_json::Map::new();
            let w = wh(p, true, &mut params);
            let sql = format!("SELECT site, COUNT(*) AS n, SUM(points) AS points FROM hn_stories {} site != '' GROUP BY site ORDER BY n DESC LIMIT 10", if w.is_empty() { "WHERE".to_string() } else { format!("{w} AND") });
            let rows = c.soft("the sites that answer it", &sql, Value::Object(params)).await;
            Some(json!({"kind": "sites", "title": format!("The sites posted the most{sc}"), "sites": rows}))
        }
        Ask::When => {
            let years: Vec<(i64, i64)> = facets.and_then(|f| f.get("year")).and_then(|y| y.as_array()).map(|a| a.iter().map(|x| (x["value"].as_i64().unwrap_or(0), x["n"].as_i64().unwrap_or(0))).collect()).unwrap_or_default();
            if years.is_empty() {
                return None;
            }
            let all = YEARS
                .get_or_init(|| async {
                    c.soft("stories a year, the whole archive", "SELECT year, COUNT(*) AS n FROM hn_stories GROUP BY year ORDER BY year", Value::Null).await.iter().map(|r| (r.i("year"), r.i("n"))).collect()
                })
                .await;
            let share = |y: i64, k: i64| -> f64 { all.iter().find(|a| a.0 == y).map(|a| k as f64 / a.1.max(1) as f64).unwrap_or(0.0) };
            let list: Vec<Value> = years.iter().map(|(y, k)| json!({"year": y, "n": k, "share": share(*y, *k)})).collect();
            // The peak by share of the year's stories (HN grew; raw counts favour late years).
            let peak = years.iter().filter(|(_, k)| *k >= 3).max_by(|a, b| share(a.0, a.1).partial_cmp(&share(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal)).copied().or(years.first().copied())?;
            let most = years.iter().max_by_key(|x| x.1).copied().unwrap_or(peak);
            let words = query::plain_words(p);
            let topic = if words.is_empty() { "It".to_string() } else { format!("“{words}”") };
            let title = format!("{topic} was talked about most in {}: {} stories, {:.2}% of that year's", peak.0, n(peak.1), 100.0 * share(peak.0, peak.1));
            let more = if most.0 != peak.0 { format!("The most stories in one year: {} in {}.", n(most.1), most.0) } else { String::new() };
            Some(json!({"kind": "when", "title": title, "peak": peak.0, "more": more, "years": list}))
        }
        Ask::Opinion => {
            let words = query::plain_words(p);
            Some(json!({"kind": "opinion", "title": format!("What HN thinks about “{words}”"), "topic": words}))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numbers_read_with_commas() {
        assert_eq!(super::n(1234567), "1,234,567");
        assert_eq!(super::n(12), "12");
    }
}
