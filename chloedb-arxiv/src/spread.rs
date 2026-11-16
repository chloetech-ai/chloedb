//! Techniques that change fields: when a method reached each arXiv
//! category after the one it grew up in, and the first papers there.
//!
//! A technique's papers are counted by primary category and year (one
//! `GROUP BY pcat, year` per technique). It arrived in a category in the
//! first year that held at least five of its papers there and a twentieth
//! of its best year there, the next year as well (so a stray paper or one
//! good year is not an arrival). Its home is the earliest arrival among
//! the categories that hold a twentieth of its papers or more: a small
//! category where the same words meant something else years before (a
//! "transformer" in instrumentation) does not count as home, and arrivals
//! before the home are left out for the same reason. The first papers in
//! a category are its most cited there in the year it arrived and the
//! next.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::RowExt;

/// A category the technique reached, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    pub cat: String,
    pub year: i64,
    pub papers: f64,
}

/// When a technique arrived in each category, from its papers by category
/// and year: (home, the arrivals after it, oldest first).
pub fn arrivals(rows: &[(String, i64, f64)]) -> Option<(Arrival, Vec<Arrival>)> {
    let mut by: HashMap<&str, HashMap<i64, f64>> = HashMap::new();
    for (cat, y, n) in rows {
        if !cat.is_empty() {
            *by.entry(cat.as_str()).or_default().entry(*y).or_default() += n;
        }
    }
    let total: f64 = by.values().flat_map(|m| m.values()).sum();
    let mut all: Vec<Arrival> = Vec::new();
    for (cat, ys) in &by {
        let papers: f64 = ys.values().sum();
        if papers < 30.0 {
            continue;
        }
        let peak = ys.values().copied().fold(0.0, f64::max);
        let thr = (peak / 20.0).max(5.0);
        let mut years: Vec<i64> = ys.keys().copied().collect();
        years.sort();
        if let Some(y) = years.iter().find(|y| ys[*y] >= thr && ys.get(&(**y + 1)).copied().unwrap_or(0.0) >= thr) {
            all.push(Arrival { cat: cat.to_string(), year: *y, papers });
        }
    }
    let home = all.iter().filter(|a| a.papers >= total / 20.0).min_by(|a, b| a.year.cmp(&b.year).then(b.papers.partial_cmp(&a.papers).unwrap_or(std::cmp::Ordering::Equal)))?.clone();
    let mut after: Vec<Arrival> = all.into_iter().filter(|a| a.cat != home.cat && a.year >= home.year).collect();
    after.sort_by(|a, b| a.year.cmp(&b.year).then(b.papers.partial_cmp(&a.papers).unwrap_or(std::cmp::Ordering::Equal)));
    Some((home, after))
}

/// Every technique's spread, those that reached most categories in the
/// last three years first.
pub async fn find(c: &Ctx<'_>) -> Value {
    let (concepts, cats) = tokio::join!(
        c.soft("the techniques", "SELECT id, name, papers FROM ax_concepts WHERE kind = 'technique' ORDER BY papers DESC LIMIT 400", Value::Null),
        c.soft("the categories' names", "SELECT id, name, area_name FROM ax_cats LIMIT 400", Value::Null),
    );
    let cat_name: HashMap<String, String> = cats.iter().map(|r| (r.s("id"), r.s("name"))).collect();
    let mut years_seen = 0i64;
    let mut out: Vec<(f64, Value)> = Vec::new();
    for chunk in concepts.chunks(12) {
        let got = futures_util::future::join_all(chunk.iter().map(|t| c.soft("its papers by category and year", "SELECT pcat, year, COUNT(*) AS n FROM ax_papers WHERE techniques = :t AND year >= 1995 GROUP BY pcat, year LIMIT 10000", json!({"t": t.s("id")})))).await;
        for (t, rows) in chunk.iter().zip(got) {
            let rows: Vec<(String, i64, f64)> = rows.iter().map(|r| (r.s("pcat"), r.i("year"), r.f("n"))).collect();
            years_seen = years_seen.max(rows.iter().map(|r| r.1).max().unwrap_or(0));
            let Some((home, after)) = arrivals(&rows) else { continue };
            out.push((0.0, json!({
                "id": t.s("id"), "name": t.s("name"), "papers": t.i("papers"),
                "home": {"cat": home.cat, "name": cat_name.get(&home.cat).cloned().unwrap_or_default(), "year": home.year, "papers": home.papers as i64},
                "hops": after.iter().map(|a| json!({"cat": a.cat, "name": cat_name.get(&a.cat).cloned().unwrap_or_default(), "year": a.year, "delay": a.year - home.year, "papers": a.papers as i64})).collect::<Vec<_>>(),
            })));
        }
    }
    // Rank by the categories reached lately, the bigger arrivals counting more.
    for (score, v) in out.iter_mut() {
        *score = v["hops"].as_array().map(|a| a.iter().filter(|h| h["year"].as_i64().unwrap_or(0) >= years_seen - 2).map(|h| (h["papers"].as_f64().unwrap_or(0.0)).ln_1p()).sum()).unwrap_or(0.0);
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    json!({"techniques": out.into_iter().map(|(_, v)| v).collect::<Vec<_>>(), "year": years_seen})
}

/// The first papers of a technique in its home and in each category it
/// reached (the most cited there in the year it arrived and the next).
pub async fn firsts(c: &Ctx<'_>, t: &str, stops: &[(String, i64)]) -> Value {
    let got = futures_util::future::join_all(stops.iter().take(16).map(|(cat, y)| {
        c.soft("its first papers in the category", &format!("SELECT {COLS} FROM ax_papers WHERE techniques = :t AND pcat = :cat AND year >= :y AND year <= :y1 ORDER BY cites DESC LIMIT 1"), json!({"t": t, "cat": cat, "y": y, "y1": y + 1}))
    }))
    .await;
    let firsts: Vec<Value> = stops.iter().zip(got).filter_map(|((cat, _), rows)| rows.into_iter().next().map(|r| json!({"cat": cat, "paper": r}))).collect();
    json!({"firsts": firsts})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(spec: &[(&str, &[(i64, f64)])]) -> Vec<(String, i64, f64)> {
        spec.iter().flat_map(|(c, ys)| ys.iter().map(move |(y, n)| (c.to_string(), *y, *n))).collect()
    }

    #[test]
    fn home_is_the_first_big_category_and_old_homonyms_are_left_out() {
        let r = rows(&[
            // An old, small use of the same word elsewhere.
            ("physics.ins-det", &[(2010, 8.0), (2011, 9.0), (2012, 10.0), (2013, 9.0)]),
            ("cs.CL", &[(2017, 20.0), (2018, 200.0), (2019, 600.0), (2020, 900.0)]),
            ("cs.CV", &[(2018, 3.0), (2019, 60.0), (2020, 800.0), (2021, 1500.0)]),
            ("q-bio.BM", &[(2020, 10.0), (2021, 40.0), (2022, 80.0)]),
            // One good year is not an arrival.
            ("astro-ph.IM", &[(2021, 40.0), (2023, 1.0)]),
        ]);
        let (home, after) = arrivals(&r).unwrap();
        // 2017's 20 papers are under a twentieth of its best year there.
        assert_eq!((home.cat.as_str(), home.year), ("cs.CL", 2018));
        let got: Vec<(&str, i64)> = after.iter().map(|a| (a.cat.as_str(), a.year)).collect();
        assert_eq!(got, vec![("cs.CV", 2020), ("q-bio.BM", 2020)]);
    }
}
