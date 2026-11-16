//! A weekly digest of Hacker News, to read instead of the front page: the
//! week's stories that drew the most (one per link), each with what it is,
//! why it mattered and what HN made of it, and the comments that carried
//! the discussion. The numbers and the picks are SQL; the prose is written
//! once per story by a model through ChloeDB's `llm()`, from the story,
//! the article's own text (fetched once, gently) and its most-replied
//! comments, and kept in `hn_answers` (kind `digest`).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, Utc};
use serde_json::{json, Value};

use crate::search::{Ctx, STORY_COLS};
use crate::strata::{Row, RowExt};
use crate::text;

/// Stories in an issue.
pub const PICKS: usize = 10;

/// The Monday of the week `day` falls in; with none, of the last full week.
pub fn monday(day: Option<&str>) -> NaiveDate {
    let today = Utc::now().date_naive();
    let d = day.and_then(|d| NaiveDate::parse_from_str(d.get(..10).unwrap_or(d), "%Y-%m-%d").ok()).unwrap_or(today - chrono::Duration::days(7));
    let d = d.min(today);
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// "Nov 9 – 15, 2026", or across months "Oct 26 – Nov 1, 2026".
pub fn label(from: NaiveDate) -> String {
    let to = from + chrono::Duration::days(6);
    if from.month() == to.month() {
        format!("{} – {}, {}", from.format("%b %-d"), to.day(), to.year())
    } else if from.year() == to.year() {
        format!("{} – {}, {}", from.format("%b %-d"), to.format("%b %-d"), to.year())
    } else {
        format!("{} – {}", from.format("%b %-d, %Y"), to.format("%b %-d, %Y"))
    }
}

fn key(from: NaiveDate, id: &str) -> String {
    format!("digest:{}:{id}", from.format("%Y-%m-%d"))
}

fn within(from: NaiveDate) -> String {
    let to = from + chrono::Duration::days(7);
    format!("created >= TIMESTAMP '{}T00:00:00Z' AND created < TIMESTAMP '{}T00:00:00Z'", from.format("%Y-%m-%d"), to.format("%Y-%m-%d"))
}

/// The week's picks: by points, one per link, no job ads.
async fn picks(c: &Ctx<'_>, from: NaiveDate) -> Vec<Row> {
    let rows = c.soft("the week's stories by points", &format!("SELECT {STORY_COLS}, curl, body FROM hn_stories WHERE {} AND kind != 'job' ORDER BY points DESC LIMIT 40", within(from)), Value::Null).await;
    let mut seen = std::collections::HashSet::new();
    rows.into_iter().filter(|r| { let k = r.s("curl"); k.is_empty() || seen.insert(k) }).take(PICKS).collect()
}

/// An issue: its week, numbers, picks with their kept prose and comments.
pub async fn issue(c: &Ctx<'_>, day: Option<&str>) -> Value {
    let from = monday(day);
    let w = within(from);
    let (picked, totals, sites, people) = tokio::join!(
        picks(c, from),
        c.soft("the week in numbers", &format!("SELECT COUNT(*) AS n, SUM(points) AS points, SUM(comments) AS comments FROM hn_stories WHERE {w}"), Value::Null),
        c.soft("the week's sites", &format!("SELECT site, COUNT(*) AS n, SUM(points) AS points FROM hn_stories WHERE {w} AND site != '' GROUP BY site ORDER BY points DESC LIMIT 8"), Value::Null),
        c.soft("the week's people", &format!("SELECT author, COUNT(*) AS n, SUM(points) AS points FROM hn_stories WHERE {w} GROUP BY author ORDER BY points DESC LIMIT 6"), Value::Null),
    );
    let mut keys: Vec<String> = picked.iter().map(|r| key(from, &r.i("id").to_string())).collect();
    keys.push(key(from, "intro"));
    let list = keys.iter().map(|k| format!("'{}'", k.replace('\'', "''"))).collect::<Vec<_>>().join(", ");
    let ids: Vec<i64> = picked.iter().map(|r| r.i("id")).collect();
    let (kept, quotes) = tokio::join!(
        c.soft("the issue's kept prose", &format!("SELECT id, answer, sources, model, at FROM hn_answers WHERE id IN ({list})"), Value::Null),
        futures_util::future::join_all(ids.iter().map(|id| c.soft("the comments that carried it", &format!("SELECT id, author, body, thread, created FROM hn_comments WHERE story = {id} AND depth = 1 ORDER BY thread DESC LIMIT 3"), Value::Null))),
    );
    let kept: std::collections::HashMap<String, &Row> = kept.iter().map(|r| (r.s("id"), r)).collect();
    let written = |k: &str| kept.get(k).and_then(|r| serde_json::from_str::<Value>(&r.s("answer")).ok());
    let entries: Vec<Value> = picked.iter().zip(quotes).map(|(s, q)| {
        let k = key(from, &s.i("id").to_string());
        let extra = kept.get(&k).and_then(|r| serde_json::from_str::<Value>(&r.s("sources")).ok()).unwrap_or(Value::Null);
        let mut story = s.clone();
        story.remove("body");
        json!({
            "story": story,
            "written": written(&k),
            "image": extra.get("image").cloned().unwrap_or(Value::Null),
            "read": extra.get("read").cloned().unwrap_or(Value::Null),
            "quotes": q.iter().map(|r| json!({"id": r.i("id"), "author": r.s("author"), "text": text::clip(&r.s("body"), 320), "replies": r.i("thread"), "created": r.s("created")})).collect::<Vec<_>>(),
        })
    }).collect();
    let t = totals.first();
    let today = Utc::now().date_naive();
    let next = from + chrono::Duration::days(7);
    json!({
        "week": from.format("%Y-%m-%d").to_string(),
        "label": label(from),
        "issue": format!("Week {}", from.iso_week().week()),
        "complete": next <= today,
        "prev": (from - chrono::Duration::days(7)).format("%Y-%m-%d").to_string(),
        "next": if next <= today { Some(next.format("%Y-%m-%d").to_string()) } else { None },
        "totals": {"stories": t.map(|r| r.i("n")).unwrap_or(0), "points": t.map(|r| r.i("points")).unwrap_or(0), "comments": t.map(|r| r.i("comments")).unwrap_or(0)},
        "sites": sites, "people": people,
        "intro": written(&key(from, "intro")),
        "entries": entries,
    })
}

pub const ENTRY: &str = "You write one entry of a weekly Hacker News digest for busy engineers who did not read HN this week. \
You get a story's title and link, the article's own text when it could be read, and the comments with the most replies under it. \
Answer with JSON only, no code fence: {\"brief\": \"two sentences on what the article or post is and says, concrete (names, numbers)\", \
\"why\": \"one sentence on why it drew HN's attention\", \
\"take\": \"three or four sentences on what HN made of it: the main views, where commenters agreed and disagreed, experiences or numbers they brought; name commenters by their username\", \
\"terms\": [\"up to three terms a newcomer would need, each as 'term: short meaning'\"]}. \
Use only what you are given. If the article text is missing, describe the story from its title and the comments without saying the article was missing, and without guessing details. Plain language, no hype.";

pub const INTRO: &str = "You open a weekly Hacker News digest. From the week's stories and their briefs, write three sentences on what the week on HN was about: the threads that ran through it and the one story to read if you read one. \
Plain prose, no list, no hype, no greeting. Answer with JSON only, no code fence: {\"intro\": \"...\"}.";

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::custom(|a| if a.previous().len() > 4 || !public_url(a.url()) { a.stop() } else { a.follow() }))
            .user_agent("hn-digest (ChloeDB demo; reads each linked article once)")
            .build()
            .expect("an HTTP client")
    })
}

/// Only pages on the public web: no addresses, no local or cluster names.
fn public_url(u: &reqwest::Url) -> bool {
    if !matches!(u.scheme(), "http" | "https") || u.port().is_some_and(|p| p != 80 && p != 443) {
        return false;
    }
    let Some(d) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    // An address, not a name (IPv6 comes in brackets).
    if d.starts_with('[') || d.parse::<std::net::IpAddr>().is_ok() || d.split('.').all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    d.contains('.') && !["localhost", ".local", ".internal", ".svc", ".lan", ".home", ".corp"].iter().any(|s| d == s.trim_start_matches('.') || d.ends_with(s))
}

/// The article's text (clipped) and its preview image, if it can be read.
pub async fn article(url: &str) -> (Option<String>, Option<String>) {
    let Ok(u) = reqwest::Url::parse(url) else { return (None, None) };
    if !public_url(&u) {
        return (None, None);
    }
    let Ok(r) = client().get(u.clone()).send().await else { return (None, None) };
    if !r.status().is_success() || !r.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.contains("html")) {
        return (None, None);
    }
    let Ok(body) = r.bytes().await else { return (None, None) };
    let html = String::from_utf8_lossy(&body[..body.len().min(3 << 20)]).to_string();
    let image = meta_image(&html).and_then(|i| u.join(&i).ok()).filter(public_url).map(|i| i.to_string());
    let txt = text::html_to_text(&strip_chrome(&html));
    let txt = txt.split_whitespace().collect::<Vec<_>>().join(" ");
    ((txt.len() > 400).then(|| text::clip(&txt, 7000)), image)
}

/// The page's `og:image` (or `twitter:image`).
fn meta_image(html: &str) -> Option<String> {
    let low = html.to_ascii_lowercase();
    for prop in ["og:image", "twitter:image"] {
        let mut at = 0;
        while let Some(i) = low[at..].find(prop) {
            let i = at + i;
            let start = low[..i].rfind('<')?;
            let end = i + low[i..].find('>')?;
            let tag = &html[start..end];
            let tl = &low[start..end];
            if tl.starts_with("<meta") {
                if let Some(c) = tl.find("content=") {
                    let rest = &tag[c + 8..];
                    let q = rest.chars().next()?;
                    let v = if q == '"' || q == '\'' { rest[1..].split(q).next() } else { rest.split([' ', '>']).next() };
                    if let Some(v) = v.map(str::trim).filter(|v| !v.is_empty()) {
                        return Some(text::unescape(v));
                    }
                }
            }
            at = end;
        }
    }
    None
}

/// The page without scripts, styles, navigation and footers, so the text
/// left is mostly the article.
fn strip_chrome(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let low = html.to_ascii_lowercase();
    let mut i = 0;
    'outer: while i < html.len() {
        for tag in ["script", "style", "nav", "footer", "header", "aside", "noscript", "svg", "form"] {
            let open = format!("<{tag}");
            if low[i..].starts_with(&open) && low[i + open.len()..].starts_with(|c: char| c == '>' || c.is_whitespace()) {
                let close = format!("</{tag}>");
                match low[i..].find(&close) {
                    Some(e) => { i += e + close.len(); continue 'outer; }
                    None => break 'outer,
                }
            }
        }
        let ch = html[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn parse_json(s: &str) -> Option<Value> {
    let a = s.find('{')?;
    let b = s.rfind('}')?;
    serde_json::from_str(&s[a..=b]).ok()
}

/// Writes (and keeps) one entry's prose, or the issue's opening with
/// `id` "intro". Only a story among the week's picks can be written.
pub async fn write(c: &Ctx<'_>, day: &str, id: &str, model: &str) -> Result<Value, String> {
    let from = monday(Some(day));
    let k = key(from, id);
    if let Some(r) = c.soft("a kept entry", "SELECT answer FROM hn_answers WHERE id = :k", json!({"k": k})).await.first() {
        if let Some(v) = parse_json(&r.s("answer")) {
            return Ok(json!({"written": v, "kept": true}));
        }
    }
    let picked = picks(c, from).await;
    let t0 = Instant::now();
    let (instructions, question, context, extra) = if id == "intro" {
        let keys = picked.iter().map(|r| format!("'{}'", key(from, &r.i("id").to_string()))).collect::<Vec<_>>().join(", ");
        let kept = c.soft("the issue's entries", &format!("SELECT id, answer FROM hn_answers WHERE id IN ({keys})"), Value::Null).await;
        let briefs: std::collections::HashMap<String, String> = kept.iter().filter_map(|r| Some((r.s("id"), parse_json(&r.s("answer"))?["brief"].as_str()?.to_string()))).collect();
        let lines: Vec<String> = picked.iter().enumerate().map(|(i, s)| {
            let b = briefs.get(&key(from, &s.i("id").to_string())).cloned().unwrap_or_default();
            format!("{}. {} ({} points, {} comments). {b}", i + 1, s.s("title"), s.i("points"), s.i("comments"))
        }).collect();
        (INTRO, format!("The week of {}", label(from)), lines.join("\n"), json!({}))
    } else {
        let Some(s) = picked.iter().find(|r| r.i("id").to_string() == id) else {
            return Err(format!("story {id} is not in the issue of {}", label(from)));
        };
        let sid = s.i("id");
        let (art, comments) = tokio::join!(
            async { if s.s("url").is_empty() { (None, None) } else { article(&s.s("url")).await } },
            c.soft("the most-replied comments", &format!("SELECT author, body, thread, depth FROM hn_comments WHERE story = {sid} ORDER BY thread DESC LIMIT 14"), Value::Null),
        );
        let (txt, image) = art;
        let mut ctx = String::new();
        if !s.s("body").is_empty() {
            ctx.push_str(&format!("The post's own text:\n{}\n\n", text::clip(&s.s("body"), 3000)));
        }
        match &txt {
            Some(t) => ctx.push_str(&format!("The article's text (start):\n{t}\n\n")),
            None if !s.s("url").is_empty() => ctx.push_str("The article could not be read.\n\n"),
            None => {}
        }
        ctx.push_str("Comments with the most replies:\n");
        for r in &comments {
            ctx.push_str(&format!("- {} ({} replies): {}\n", r.s("author"), r.i("thread"), text::clip(&r.s("body"), 700)));
        }
        (ENTRY, format!("Story: {}\nLink: {}\n{} points, {} comments", s.s("title"), s.s("url"), s.i("points"), s.i("comments")), ctx, json!({"image": image, "read": txt.is_some()}))
    };
    // A long thread can run an answer past its budget, which cuts the JSON:
    // asked again once, with more room.
    let mut v = None;
    for budget in [1400, 2200] {
        let sql = format!("SELECT id, llm('{}', :instructions, :question, :context, max_tokens => {budget}) AS answer FROM hn_stories LIMIT 1", model.replace('\'', "''"));
        let got = c.rows("the entry (llm() on the server)", &sql, json!({"instructions": instructions, "question": question, "context": context})).await.map_err(|e| e.to_string())?;
        v = got.first().and_then(|r| parse_json(&r.s("answer")));
        if v.is_some() {
            break;
        }
    }
    let v = v.ok_or_else(|| "the model's answer was not the entry asked for: try again later".to_string())?;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let _ = c.db.write(vec![json!({"op": "upsert", "table": "hn_answers", "doc": {"id": k, "kind": "digest", "question": question, "answer": v.to_string(), "sources": extra.to_string(), "model": model, "at": at, "ms": ms}})]).await;
    Ok(json!({"written": v, "kept": false, "image": extra.get("image"), "read": extra.get("read"), "ms": ms}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(monday(Some("2026-11-15")).to_string(), "2026-11-09");
        assert_eq!(monday(Some("2026-11-09")).to_string(), "2026-11-09");
        assert_eq!(label(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()), "Nov 9 – 15, 2026");
        assert_eq!(label(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap()), "Oct 26 – Nov 1, 2026");
    }

    #[test]
    fn only_public_pages_are_read() {
        let ok = |u: &str| public_url(&reqwest::Url::parse(u).unwrap());
        assert!(ok("https://example.com/a"));
        assert!(!ok("http://localhost/x"));
        assert!(!ok("http://10.0.0.1/x"));
        assert!(!ok("http://hn4.strata.svc.cluster.local:7700/v1/query"));
        assert!(!ok("http://hn4.strata.svc/x"));
        assert!(!ok("https://example.com:8443/x"));
        assert!(!ok("file:///etc/passwd"));
    }

    #[test]
    fn preview_images() {
        assert_eq!(meta_image(r#"<head><meta property="og:image" content="https://x.com/a.png"></head>"#).as_deref(), Some("https://x.com/a.png"));
        assert_eq!(meta_image(r#"<meta content='/b.jpg' name='twitter:image'>"#).as_deref(), Some("/b.jpg"));
        assert_eq!(meta_image(r#"<meta name="twitter:image" content="/b.jpg">"#).as_deref(), Some("/b.jpg"));
        assert!(strip_chrome("<p>a</p><script>x</script><nav class=n>menu</nav><p>b</p>").contains("<p>b</p>"));
        assert!(!strip_chrome("<script>x</script>").contains('x'));
    }
}
