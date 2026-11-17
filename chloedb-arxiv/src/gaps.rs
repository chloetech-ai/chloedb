//! The gap finder: rising techniques that have barely reached a domain
//! that the techniques most like them already work in.
//!
//! Over the last three years, a technique's papers are counted by domain
//! (one `GROUP BY domains` per technique) and by area. Two techniques
//! are alike when their domain counts point the same way (cosine, the
//! domain in question left out). A technique's expected papers in a
//! domain are its papers times the share of the domain among its eight
//! closest look-alikes, weighted by the square of how alike they are. A
//! gap is a domain with a quarter of the expected papers or fewer, where
//! the technique and the domain both grow faster than the median of
//! their kind, their areas overlap (cosine 0.5 or more, so a method of
//! language models is not sent to black holes), and two of the three
//! closest look-alikes use the domain more than arXiv does. Gaps are
//! ranked by how far below expected they are, (expected − seen) / √expected.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::search::Ctx;
use crate::strata::RowExt;

/// The window counted.
pub const WINDOW: &str = "created >= now() - INTERVAL '1095 days'";

type Counts = HashMap<String, f64>;

fn cos(a: &Counts, b: &Counts, skip: Option<&str>) -> f64 {
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for (k, x) in a {
        if Some(k.as_str()) == skip {
            continue;
        }
        da += x * x;
        if let Some(y) = b.get(k) {
            num += x * y;
        }
    }
    for (k, y) in b {
        if Some(k.as_str()) != skip {
            db += y * y;
        }
    }
    if da > 0.0 && db > 0.0 {
        num / (da.sqrt() * db.sqrt())
    } else {
        0.0
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v.get(v.len() / 2).copied().unwrap_or(1.0)
}

/// Every gap, best first. About 320 small queries, 12 at a time.
pub async fn find(c: &Ctx<'_>) -> Value {
    let concepts = c.soft("the techniques and domains", "SELECT id, kind, name, growth FROM ax_concepts", Value::Null).await;
    let name: HashMap<String, String> = concepts.iter().map(|r| (r.s("id"), r.s("name"))).collect();
    let growth: HashMap<String, f64> = concepts.iter().map(|r| (r.s("id"), r.f("growth"))).collect();
    let techs: Vec<String> = concepts.iter().filter(|r| r.s("kind") == "technique").map(|r| r.s("id")).collect();
    let doms: Vec<String> = concepts.iter().filter(|r| r.s("kind") == "domain").map(|r| r.s("id")).collect();
    let (tn, dn, all) = tokio::join!(
        c.soft("papers by technique", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers WHERE {WINDOW} GROUP BY techniques ORDER BY n DESC LIMIT 400"), Value::Null),
        c.soft("papers by domain", &format!("SELECT domains, COUNT(*) AS n FROM ax_papers WHERE {WINDOW} GROUP BY domains ORDER BY n DESC LIMIT 400"), Value::Null),
        c.soft("all papers", &format!("SELECT COUNT(*) AS n FROM ax_papers WHERE {WINDOW}"), Value::Null),
    );
    let nt: Counts = tn.iter().map(|r| (r.s("techniques"), r.f("n"))).collect();
    let nd: Counts = dn.iter().map(|r| (r.s("domains"), r.f("n"))).collect();
    let n_all = all.first().map(|r| r.f("n")).unwrap_or(0.0);
    if n_all <= 0.0 || techs.is_empty() {
        return json!({"gaps": [], "note": "no techniques counted yet"});
    }
    // Domains by technique, areas by technique and by domain.
    let mut m: HashMap<String, Counts> = HashMap::new();
    for chunk in techs.chunks(12) {
        let got = futures_util::future::join_all(chunk.iter().map(|t| c.soft("its papers by domain", &format!("SELECT domains, COUNT(*) AS n FROM ax_papers WHERE techniques = :t AND {WINDOW} GROUP BY domains ORDER BY n DESC LIMIT 100"), json!({"t": t})))).await;
        for (t, rows) in chunk.iter().zip(got) {
            m.insert(t.clone(), rows.iter().filter(|r| !r.s("domains").is_empty()).map(|r| (r.s("domains"), r.f("n"))).collect());
        }
    }
    let keys: Vec<(String, &str)> = techs.iter().map(|t| (t.clone(), "techniques")).chain(doms.iter().map(|d| (d.clone(), "domains"))).collect();
    let mut areas: HashMap<String, Counts> = HashMap::new();
    for chunk in keys.chunks(12) {
        let got = futures_util::future::join_all(chunk.iter().map(|(k, col)| c.soft("its papers by area", &format!("SELECT area, COUNT(*) AS n FROM ax_papers WHERE {col} = :k AND {WINDOW} GROUP BY area ORDER BY n DESC LIMIT 40"), json!({"k": k})))).await;
        for ((k, _), rows) in chunk.iter().zip(got) {
            areas.insert(k.clone(), rows.iter().filter(|r| !r.s("area").is_empty()).map(|r| (r.s("area"), r.f("n"))).collect());
        }
    }
    let gt = median(techs.iter().map(|t| growth.get(t).copied().unwrap_or(1.0)).collect());
    let gd = median(doms.iter().map(|d| growth.get(d).copied().unwrap_or(1.0)).collect());
    let empty = Counts::new();
    let mut gaps: Vec<(f64, Value)> = Vec::new();
    for t in &techs {
        let n_t = nt.get(t).copied().unwrap_or(0.0);
        if n_t < 300.0 || growth.get(t).copied().unwrap_or(0.0) < gt {
            continue;
        }
        let rt = m.get(t).unwrap_or(&empty);
        for d in &doms {
            if growth.get(d).copied().unwrap_or(0.0) < gd {
                continue;
            }
            if cos(areas.get(t).unwrap_or(&empty), areas.get(d).unwrap_or(&empty), None) < 0.5 {
                continue;
            }
            let mut like: Vec<(f64, &String)> = m.iter().filter(|(t2, _)| *t2 != t && nt.get(*t2).copied().unwrap_or(0.0) >= 100.0).map(|(t2, r2)| (cos(rt, r2, Some(d)), t2)).filter(|(s, _)| *s > 0.5).collect();
            like.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            like.truncate(8);
            if like.len() < 3 {
                continue;
            }
            let share_of = |t2: &String| m.get(t2).and_then(|r| r.get(d)).copied().unwrap_or(0.0) / nt.get(t2).copied().unwrap_or(1.0).max(1.0);
            let base = nd.get(d).copied().unwrap_or(0.0) / n_all;
            if like.iter().take(3).filter(|(_, t2)| share_of(t2) >= base).count() < 2 {
                continue;
            }
            let w: f64 = like.iter().map(|(s, _)| s * s).sum();
            let share = like.iter().map(|(s, t2)| s * s * share_of(t2)).sum::<f64>() / w;
            let pred = n_t * share;
            let obs = rt.get(d).copied().unwrap_or(0.0);
            if pred < 25.0 || obs * 4.0 > pred {
                continue;
            }
            let z = (pred - obs) / pred.sqrt();
            let likes: Vec<Value> = like.iter().take(3).map(|(s, t2)| json!({"id": t2, "name": name.get(*t2).cloned().unwrap_or((*t2).clone()), "n": m.get(*t2).and_then(|r| r.get(d)).copied().unwrap_or(0.0) as i64, "share": (share_of(t2) * 1000.0).round() / 10.0, "alike": (s * 100.0).round() / 100.0})).collect();
            gaps.push((z, json!({
                "t": t, "t_name": name.get(t).cloned().unwrap_or(t.clone()), "d": d, "d_name": name.get(d).cloned().unwrap_or(d.clone()),
                "seen": obs as i64, "expected": pred.round() as i64, "z": (z * 10.0).round() / 10.0, "t_papers": n_t as i64,
                "t_growth": growth.get(t), "d_growth": growth.get(d), "like": likes,
            })));
        }
    }
    gaps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    json!({"gaps": gaps.into_iter().take(80).map(|(_, v)| v).collect::<Vec<_>>(), "techniques": techs.len(), "domains": doms.len(), "papers": n_all as i64})
}

/// One gap: its few papers, what the look-alikes did in the domain, and
/// the people who work on both (among the 300 who write most on each).
pub async fn one(c: &Ctx<'_>, t: &str, d: &str, likes: &[String]) -> Value {
    let p = json!({"t": t, "d": d});
    let cols = crate::search::COLS;
    let (both, by_t, by_d) = tokio::join!(
        c.soft("the papers that already combine them", &format!("SELECT {cols} FROM ax_papers WHERE techniques = :t AND domains = :d ORDER BY cites DESC LIMIT 6"), p.clone()),
        c.soft("who writes most with the technique", &format!("SELECT people, COUNT(*) AS n FROM ax_papers WHERE techniques = :t AND {WINDOW} GROUP BY people ORDER BY n DESC LIMIT 300"), p.clone()),
        c.soft("who writes most in the domain", &format!("SELECT people, COUNT(*) AS n FROM ax_papers WHERE domains = :d AND {WINDOW} GROUP BY people ORDER BY n DESC LIMIT 300"), p.clone()),
    );
    let near = futures_util::future::join_all(likes.iter().take(3).map(|l| {
        c.soft("what a look-alike did in the domain", &format!("SELECT {cols} FROM ax_papers WHERE techniques = :l AND domains = :d AND {WINDOW} ORDER BY cites DESC LIMIT 2"), json!({"l": l, "d": d}))
    }))
    .await;
    let dmap: HashMap<String, i64> = by_d.iter().map(|r| (r.s("people"), r.i("n"))).collect();
    let mut people: Vec<(f64, String, i64, i64)> = by_t.iter().filter_map(|r| {
        let k = r.s("people");
        let a = r.i("n");
        dmap.get(&k).map(|b| (((a * b) as f64).sqrt(), k, a, *b))
    }).filter(|x| !x.1.is_empty()).collect();
    people.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    people.truncate(10);
    let keys: Vec<String> = people.iter().map(|x| x.1.clone()).collect();
    let names = crate::search::names(c, &keys).await;
    let people: Vec<Value> = people.iter().map(|(_, k, a, b)| json!({"id": k, "name": names.get(k).map(|r| r.s("name")).unwrap_or(k.clone()), "with_t": a, "in_d": b})).collect();
    let near: Vec<Value> = likes.iter().zip(near.iter()).flat_map(|(l, rows)| rows.iter().map(move |r| json!({"like": l, "paper": r}))).collect();
    json!({"papers": both, "near": near, "people": people})
}
