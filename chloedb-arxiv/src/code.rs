//! Code and data behind the papers: how many share their code, year by
//! year, in all of arXiv or an area; the repositories behind the most
//! papers; the techniques whose papers share code most and least; and the
//! newest and most cited papers with code, and those with models or data on
//! Hugging Face. The links are read from each paper's abstract and comments.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::RowExt;

pub async fn page(c: &Ctx<'_>, area: &str) -> Value {
    let f = if area.is_empty() { String::new() } else { " AND areas = :area".to_string() };
    let p = json!({"area": area});
    let code = "code LIKE 'https%'";
    let (all, with, repos, tall, tcode, newest, cited, hf) = tokio::join!(
        c.soft("papers a year", &format!("SELECT year, COUNT(*) AS n FROM ax_papers WHERE year >= 2012{f} GROUP BY year ORDER BY year"), p.clone()),
        c.soft("papers with code a year", &format!("SELECT year, COUNT(*) AS n FROM ax_papers WHERE year >= 2012 AND {code}{f} GROUP BY year ORDER BY year"), p.clone()),
        c.soft("the repositories behind the most papers", &format!("SELECT code, COUNT(*) AS n FROM ax_papers WHERE {code}{f} GROUP BY code ORDER BY n DESC LIMIT 20"), p.clone()),
        c.soft("papers per technique (two years)", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers WHERE created >= now() - INTERVAL '730 days'{f} GROUP BY techniques ORDER BY n DESC LIMIT 80"), p.clone()),
        c.soft("papers with code per technique (two years)", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers WHERE created >= now() - INTERVAL '730 days' AND {code}{f} GROUP BY techniques ORDER BY n DESC LIMIT 300"), p.clone()),
        c.soft("the newest papers with code", &format!("SELECT {COLS} FROM ax_papers WHERE {code}{f} ORDER BY created DESC LIMIT 10"), p.clone()),
        c.soft("the most cited papers with code of the last year", &format!("SELECT {COLS} FROM ax_papers WHERE created >= now() - INTERVAL '365 days' AND {code}{f} ORDER BY cites DESC LIMIT 10"), p.clone()),
        c.soft("the newest papers with models or data on Hugging Face", &format!("SELECT {COLS} FROM ax_papers WHERE hf LIKE 'https%'{f} ORDER BY created DESC LIMIT 8"), p.clone()),
    );
    let with_y: HashMap<i64, i64> = with.iter().map(|r| (r.i("year"), r.i("n"))).collect();
    let years: Vec<Value> = all.iter().map(|r| {
        let (y, n) = (r.i("year"), r.i("n"));
        let k = with_y.get(&y).copied().unwrap_or(0);
        json!({"year": y, "n": n, "code": k, "share": if n > 0 { k as f64 / n as f64 } else { 0.0 }})
    }).collect();
    let tc: HashMap<String, i64> = tcode.iter().map(|r| (r.s("techniques"), r.i("n"))).collect();
    let dict = crate::concepts::dict();
    let mut shares: Vec<(String, i64, i64, f64)> = tall
        .iter()
        .map(|r| (r.s("techniques"), r.i("n")))
        .filter(|(t, n)| !t.is_empty() && *n >= 300)
        .map(|(t, n)| {
            let k = tc.get(&t).copied().unwrap_or(0);
            (t, n, k, k as f64 / n as f64)
        })
        .collect();
    shares.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let row = |(t, n, k, s): &(String, i64, i64, f64)| json!({"id": t, "name": dict.by_slug(t).map(|c| c.name.clone()).unwrap_or(t.clone()), "n": n, "code": k, "share": s});
    let most: Vec<Value> = shares.iter().take(10).map(row).collect();
    let least: Vec<Value> = shares.iter().rev().take(10).map(row).collect();
    json!({
        "area": area, "years": years,
        "repos": repos.iter().map(|r| json!({"url": r.s("code"), "n": r.i("n")})).collect::<Vec<_>>(),
        "most": most, "least": least, "newest": newest, "cited": cited, "hf": hf,
    })
}
