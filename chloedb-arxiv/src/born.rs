//! Topics born lately: topics that barely existed and now hold hundreds
//! of papers, each with the paper that started it and where it is on its
//! curve.
//!
//! A topic is a candidate when most of its papers are from the last three
//! years. Its papers are counted by month over five years (one
//! `GROUP BY topic, month` for many topics) and smoothed over three
//! months. It was born in the first month its count reached a fifth of its
//! peak, provided the year before held an eighth of the peak or less (an
//! old topic that merely grew is not new). The paper that started it is
//! the most cited of its papers from six months before it was born to two
//! after. Its state compares its share of arXiv's papers over the last six
//! full months with the six before (conference deadlines make single
//! months jump; a topic younger than a year compares the halves of its
//! months): rising (a quarter more or better), fading (a fifth less
//! or worse), at its peak (neither, and never higher), or steady.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::RowExt;

/// "2026-03" plus `k` months.
pub fn add_months(m: &str, k: i32) -> String {
    let (y, mo) = m.split_once('-').and_then(|(y, mo)| Some((y.parse::<i32>().ok()?, mo.parse::<i32>().ok()?))).unwrap_or((1970, 1));
    let n = y * 12 + (mo - 1) + k;
    format!("{}-{:02}", n.div_euclid(12), n.rem_euclid(12) + 1)
}

/// Where a topic is on its curve, from its monthly shares (oldest first,
/// full months only).
pub fn state(shares: &[f64]) -> (&'static str, f64) {
    let n = shares.len();
    // Six months against six, or halves of a younger topic's months.
    let w = (n / 2).min(6);
    if w < 2 {
        return ("new", 1.0);
    }
    let last: f64 = shares[n - w..].iter().sum();
    let before: f64 = shares[n - 2 * w..n - w].iter().sum();
    let r = if before > 0.0 { last / before } else { 9.99 };
    let best = (0..=n - w).map(|i| shares[i..i + w].iter().sum::<f64>()).fold(0.0, f64::max);
    let s = if r >= 1.25 {
        "rising"
    } else if r <= 0.8 {
        "fading"
    } else if last >= best * 0.999 {
        "at its peak"
    } else {
        "steady"
    };
    (s, (r * 100.0).round() / 100.0)
}

/// The month a topic was born in, from its monthly counts (oldest first):
/// the first month its three-month average reached a fifth of its peak,
/// when the twelve months before averaged an eighth of the peak or less.
pub fn born(months: &[String], counts: &[f64]) -> Option<String> {
    let n = counts.len();
    let smooth: Vec<f64> = (0..n).map(|i| counts[i.saturating_sub(2)..=i].iter().sum::<f64>() / ((i - i.saturating_sub(2) + 1) as f64)).collect();
    let peak = smooth.iter().copied().fold(0.0, f64::max);
    if peak < 10.0 {
        return None;
    }
    let i = smooth.iter().position(|x| *x >= peak / 5.0)?;
    let before = &counts[i.saturating_sub(12)..i];
    let mean = if before.is_empty() { 0.0 } else { before.iter().sum::<f64>() / before.len() as f64 };
    // Born before the window began, or grew out of something already there.
    if i < 6 || mean > peak / 8.0 {
        return None;
    }
    // The first month of the three averaged.
    Some(months[i.saturating_sub(2)].clone())
}

/// Every topic born in the last three years, the biggest first.
pub async fn find(c: &Ctx<'_>) -> Value {
    let (topics, all) = tokio::join!(
        c.soft("the topics", "SELECT id, label, about, field_label, papers, recent, by_year FROM ax_topics LIMIT 5000", Value::Null),
        c.soft("all papers by month", "SELECT month, COUNT(*) AS n FROM ax_papers WHERE created >= now() - INTERVAL '1830 days' GROUP BY month ORDER BY month LIMIT 100", Value::Null),
    );
    let mut months: Vec<String> = all.iter().map(|r| r.s("month")).filter(|m| !m.is_empty()).collect();
    months.sort();
    // The last month is still filling up.
    months.pop();
    let Some(last) = months.last().cloned() else { return json!({"topics": [], "note": "no papers counted"}) };
    let total: HashMap<String, f64> = all.iter().map(|r| (r.s("month"), r.f("n"))).collect();
    let year: i32 = last[..4].parse().unwrap_or(2026);
    let cands: Vec<i64> = topics
        .iter()
        .filter(|t| {
            let by: HashMap<String, f64> = serde_json::from_str(&t.s("by_year")).unwrap_or_default();
            let all: f64 = by.values().sum();
            let late: f64 = (0..3).map(|k| by.get(&(year - k).to_string()).copied().unwrap_or(0.0)).sum();
            all >= 150.0 && late >= all * 0.6
        })
        .map(|t| t.i("id"))
        .collect();
    let mut by: HashMap<i64, HashMap<String, f64>> = HashMap::new();
    for chunk in cands.chunks(150) {
        let rows = c.soft("their papers by month", "SELECT topic, month, COUNT(*) AS n FROM ax_papers WHERE topic IN (:ts) AND created >= now() - INTERVAL '1830 days' GROUP BY topic, month LIMIT 10000", json!({"ts": chunk})).await;
        for r in rows {
            by.entry(r.i("topic")).or_default().insert(r.s("month"), r.f("n"));
        }
    }
    let first = add_months(&last, -35);
    let mut out: Vec<(f64, Value, String)> = Vec::new();
    for t in &topics {
        let id = t.i("id");
        let Some(m) = by.get(&id) else { continue };
        let counts: Vec<f64> = months.iter().map(|k| m.get(k).copied().unwrap_or(0.0)).collect();
        let Some(b) = born(&months, &counts) else { continue };
        if b < first {
            continue;
        }
        let shares: Vec<f64> = months.iter().zip(&counts).map(|(k, n)| n / total.get(k).copied().unwrap_or(1.0).max(1.0)).collect();
        let from = months.iter().position(|k| *k == b).unwrap_or(0);
        let (st, ratio) = state(&shares[from..]);
        let last12: f64 = counts[counts.len().saturating_sub(12)..].iter().sum();
        let series: Vec<Value> = months.iter().zip(&counts).zip(&shares).skip(from.saturating_sub(12)).map(|((k, n), s)| json!([k, *n as i64, (s * 1e6).round() / 100.0])).collect();
        out.push((last12, json!({
            "id": id, "label": t.s("label"), "about": t.s("about"), "field": t.s("field_label"),
            "born": b, "papers": t.i("papers"), "last12": last12 as i64, "state": st, "ratio": ratio, "months": series,
        }), b));
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(90);
    // The paper that started each: the most cited of its first months.
    let mut started: Vec<Vec<Value>> = Vec::new();
    for chunk in out.chunks(12) {
        let got = futures_util::future::join_all(chunk.iter().map(|(_, v, b)| {
            let since = format!("{}-01T00:00:00Z", add_months(b, -6));
            let until = format!("{}-01T00:00:00Z", add_months(b, 3));
            c.soft("the papers that started it", &format!("SELECT {COLS} FROM ax_papers WHERE topic = :t AND created >= :since AND created < :until ORDER BY cites DESC LIMIT 2"), json!({"t": v["id"], "since": since, "until": until}))
        }))
        .await;
        started.extend(got.into_iter().map(|rows| rows.into_iter().map(|r| json!(r)).collect()));
    }
    let topics: Vec<Value> = out.into_iter().zip(started).map(|((_, mut v, _), s)| {
        v["started"] = json!(s);
        v
    }).collect();
    json!({"topics": topics, "last_month": last, "candidates": cands.len()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_add_across_years() {
        assert_eq!(add_months("2026-11", -23), "2024-12");
        assert_eq!(add_months("2025-12", 1), "2026-01");
        assert_eq!(add_months("2026-01", -1), "2025-12");
    }

    #[test]
    fn a_topic_is_born_once_it_reaches_a_fifth_of_its_peak() {
        let months: Vec<String> = (0..24).map(|i| add_months("2024-11", i)).collect();
        // Nothing for a year, then it takes off.
        let mut counts = vec![1.0; 12];
        counts.extend([4.0, 10.0, 25.0, 40.0, 60.0, 80.0, 100.0, 120.0, 130.0, 140.0, 150.0, 160.0]);
        let b = born(&months, &counts).unwrap();
        assert!(b.as_str() >= "2025-11" && b.as_str() <= "2026-02", "{b}");
        // An old topic that grew is not new.
        let old: Vec<f64> = (0..24).map(|i| 40.0 + i as f64 * 5.0).collect();
        assert_eq!(born(&months, &old), None);
    }

    #[test]
    fn where_a_topic_is_on_its_curve() {
        let six = |a: f64, b: f64| [vec![a; 6], vec![b; 6]].concat();
        assert_eq!(state(&six(1.0, 2.0)).0, "rising");
        assert_eq!(state(&six(3.0, 1.0)).0, "fading");
        assert_eq!(state(&[vec![1.0, 2.0], six(3.0, 3.1)].concat()).0, "at its peak");
        assert_eq!(state(&[vec![5.0; 6], six(4.0, 4.0)].concat()).0, "steady");
        assert_eq!(state(&[1.0, 1.0, 2.0, 2.0]).0, "rising");
        assert_eq!(state(&[1.0, 2.0, 3.0]).0, "new");
    }
}
