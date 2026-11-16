//! Trends: how much HN talked about something, year by year or month by
//! month, as a share of everything said then (so the site's growth does
//! not read as interest), with the story that peaked each term. One
//! `GROUP BY` over the rows a `MATCH` finds, per term.

use serde_json::{json, Value};

use crate::search::{Ctx, STORY_COLS};
use crate::strata::RowExt;

pub async fn trends(c: &Ctx<'_>, terms: &[String], by_month: bool, comments: bool) -> Value {
    let (table, field) = if comments { ("hn_comments", "body") } else { ("hn_stories", "title") };
    let period = if by_month { "date_trunc('month', created)" } else { "year" };
    let totals = c.soft("everything said, per period", &format!("SELECT {period} AS p, COUNT(*) AS n FROM {table} GROUP BY p ORDER BY {period}"), Value::Null).await;
    let total: std::collections::HashMap<String, i64> = totals.iter().map(|r| (r.s("p"), r.i("n"))).collect();
    let mut series = Vec::new();
    for t in terms.iter().take(6) {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        let m = crate::query::match_text(&crate::query::parse(t, 0));
        let params = json!({"m": m});
        let (rows, peak) = tokio::join!(
            c.soft(&format!("“{t}”, per period"), &format!("SELECT {period} AS p, COUNT(*) AS n FROM {table} WHERE MATCH({field}, :m, synonyms => 1) GROUP BY p ORDER BY {period}"), params.clone()),
            c.soft(&format!("“{t}”: the stories that drew most points"), &format!("SELECT {STORY_COLS} FROM hn_stories WHERE MATCH(title, :m, synonyms => 1) ORDER BY points DESC LIMIT 8"), params.clone()),
        );
        let pts: Vec<Value> = rows.iter().map(|r| {
            let p = r.s("p");
            let n = r.i("n");
            let all = total.get(&p).copied().unwrap_or(0).max(1);
            json!({"p": p, "n": n, "per10k": (n as f64 * 10_000.0 / all as f64 * 100.0).round() / 100.0})
        }).collect();
        series.push(json!({"term": t, "points": pts, "total": rows.iter().map(|r| r.i("n")).sum::<i64>(), "top": peak}));
    }
    json!({"series": series, "by": if by_month { "month" } else { "year" }, "in": if comments { "comments" } else { "stories" }, "totals": totals})
}
