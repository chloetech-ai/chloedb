//! The HN canon: the books and papers Hacker News recommends most.
//!
//! `hn_canon` holds a row a work: the books linked from comments (Amazon
//! by ISBN, Goodreads), merged across editions by their title and first
//! author, and the arXiv papers, each counted by everyone who linked to it
//! or named it in a comment, with those people a year. A comment names a
//! work when it uses its short name (SICP), writes its title as a title
//! (each word capitalised: "Getting Things Done", not "getting things
//! done") beside a word about books or papers, or writes the title beside
//! its author's name, the only way for a title of one or two words ("Steve
//! Jobs", "Deep Learning"); a paper counts from the year of its arXiv id. `canon/build.py` makes the rows: each
//! keeps the statement that found its candidates, the rule that kept them
//! and how many each part of the rule kept, which the work's page shows.

use serde_json::{json, Value};

use crate::search::Ctx;
use crate::strata::{Row, RowExt};

const COLS: &str = "id, kind, title, creator, published, url, image, people, mentions, linkers, first, last, years";

/// People a year, from the row's `years` (`{"2014": {"n": 3, "people": 2}}`).
fn years(r: &Row) -> Vec<(i64, i64)> {
    let v: Value = serde_json::from_str(&r.s("years")).unwrap_or(Value::Null);
    let mut out: Vec<(i64, i64)> = v.as_object().into_iter().flatten().filter_map(|(y, x)| Some((y.parse().ok()?, x["people"].as_i64()?))).collect();
    out.sort();
    out
}

fn item(r: &Row, from: i64, to: i64) -> Value {
    let ys = years(r);
    let within: i64 = ys.iter().filter(|(y, _)| *y >= from && *y <= to).map(|x| x.1).sum();
    let peak = ys.iter().max_by_key(|x| x.1).map(|x| x.0);
    json!({
        "id": r.s("id"), "kind": r.s("kind"), "title": r.s("title"), "creator": r.s("creator"), "published": r.s("published"),
        "url": r.s("url"), "image": r.s("image"), "people": r.i("people"), "mentions": r.i("mentions"), "linkers": r.i("linkers"),
        "first": r.i("first"), "last": r.i("last"), "peak": peak, "within": within,
        "years": ys.iter().map(|(y, n)| json!({"year": y, "people": n})).collect::<Vec<_>>(),
    })
}

/// The canon of a kind (`book` or `paper`), for the years `from` to `to`
/// (by the people of those years), or matching `q` in the title.
pub async fn list(c: &Ctx<'_>, kind: &str, from: Option<i64>, to: Option<i64>, q: &str) -> Value {
    let kind = if kind == "paper" { "paper" } else { "book" };
    let mut params = json!({"k": kind});
    let mut wh = "kind = :k".to_string();
    if !q.trim().is_empty() {
        params["q"] = json!(q.trim());
        wh.push_str(" AND (MATCH(title, :q, fuzzy => 'auto') OR MATCH(creator, :q))");
    }
    let rows = c.soft("the canon", &format!("SELECT {COLS} FROM hn_canon WHERE {wh} ORDER BY people DESC LIMIT 1000"), params).await;
    let (f, t) = (from.unwrap_or(0), to.unwrap_or(9999));
    let mut items: Vec<Value> = rows.iter().map(|r| item(r, f, t)).filter(|v| (from.is_none() && to.is_none()) || v["within"].as_i64() > Some(0)).collect();
    if from.is_some() || to.is_some() {
        items.sort_by(|a, b| b["within"].as_i64().cmp(&a["within"].as_i64()));
    }
    items.truncate(150);
    let total = c.soft("how many works", "SELECT kind, COUNT(*) AS n FROM hn_canon GROUP BY kind", Value::Null).await;
    json!({"kind": kind, "from": from, "to": to, "q": q, "works": items, "counts": total.iter().map(|r| (r.s("kind"), json!(r.i("n")))).collect::<serde_json::Map<_, _>>()})
}

/// One work: its row, the comments that named it best received first,
/// with the name marked, and the condition it was counted by.
pub async fn work(c: &Ctx<'_>, id: &str) -> Option<Value> {
    let r = c.soft("the work", &format!("SELECT {COLS}, comments, phrase, params, ids, rule, candidates, why FROM hn_canon WHERE id = :id"), json!({"id": id})).await.into_iter().next()?;
    let ids: Vec<i64> = serde_json::from_str(&r.s("comments")).unwrap_or_default();
    let mut comments = if ids.is_empty() {
        Vec::new()
    } else {
        let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        c.soft("the comments that named it, best received first", &format!("SELECT id, author, created, body, story, story_title, pop FROM hn_comments WHERE id IN ({list}) ORDER BY pop DESC"), Value::Null).await
    };
    // The words to mark: the title, and the names it goes by.
    let params: Value = serde_json::from_str(&r.s("params")).unwrap_or(Value::Null);
    let marks: Vec<String> = params.as_object().into_iter().flatten().filter(|(k, _)| k.as_str() != "a").filter_map(|(_, v)| v.as_str().map(|s| s.to_string())).collect();
    let bodies: Vec<String> = comments.iter().map(|x| x.s("body")).collect();
    if let Some(hl) = crate::search::highlight(c, "hn_comments", "body", &marks.join(" OR "), bodies, 480).await {
        for (x, h) in comments.iter_mut().zip(hl) {
            x.insert("body_hl".into(), json!(h));
        }
    }
    let mut v = item(&r, 0, 9999);
    v["comments"] = json!(comments);
    v["candidates_by"] = json!(format!("SELECT id, author, year, body FROM hn_comments WHERE {}", r.s("phrase")));
    v["counted_params"] = params;
    v["rule"] = json!(r.s("rule"));
    v["candidates"] = json!(r.i("candidates"));
    v["why"] = serde_json::from_str(&r.s("why")).unwrap_or(Value::Null);
    v["editions"] = serde_json::from_str(&r.s("ids")).unwrap_or(Value::Null);
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_by_year_and_within_years() {
        let r: Row = json!({"id": "book:x", "kind": "book", "title": "X", "people": 9, "years": r#"{"2015": {"n": 4, "people": 3}, "2012": {"n": 2, "people": 2}, "2020": {"n": 9, "people": 4}}"#}).as_object().unwrap().clone();
        assert_eq!(years(&r), vec![(2012, 2), (2015, 3), (2020, 4)]);
        let v = item(&r, 2013, 2019);
        assert_eq!(v["within"], 3);
        assert_eq!(v["peak"], 2020);
    }
}
