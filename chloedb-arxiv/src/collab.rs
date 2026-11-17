//! People to work with: those whose recent work is closest to a person's
//! in meaning but who are three or more steps away from them among
//! coauthors, with the techniques they would bring and the papers that make
//! them close.
//!
//! The person's work is the text of their eight newest papers and their
//! eight most cited of the last six years, embedded where the query runs;
//! the 500 nearest papers of the last four years (of 30 authors or fewer)
//! are read, and each of their authors scores by the ranks of their papers
//! there (two papers at least), a little more for an h-index. Distance
//! comes from the papers themselves: one step for anyone they wrote with,
//! two for anyone who wrote with one of those in the last six years
//! (papers of more than 50 authors left out); three when one of their
//! coauthors writes repeatedly (an edge of `ax_coauthors`, two papers or
//! more) with one of the candidate's.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::{Row, RowExt};

/// Scores people by the ranks of their papers in a list of the nearest
/// papers (each paper's `people`), skipping `me`; those with two papers or
/// more, best first.
pub fn by_ranks(papers: &[(String, Vec<String>)], me: &str) -> Vec<(String, f64, Vec<String>)> {
    let mut score: HashMap<&str, (f64, Vec<String>)> = HashMap::new();
    for (rank, (id, people)) in papers.iter().enumerate() {
        if people.iter().any(|p| p == me) {
            continue;
        }
        for p in people {
            let e = score.entry(p.as_str()).or_insert((0.0, Vec::new()));
            e.0 += 1.0 / (20.0 + rank as f64);
            e.1.push(id.clone());
        }
    }
    let mut out: Vec<(String, f64, Vec<String>)> = score.into_iter().filter(|(_, v)| v.1.len() >= 2).map(|(k, v)| (k.to_string(), v.0, v.1)).collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    out
}

pub async fn find(c: &Ctx<'_>, me: &str) -> Option<Value> {
    let p = json!({"a": me});
    let (row, newest, cited, all_mine) = tokio::join!(
        c.soft("the person", "SELECT id, name, techniques, cats FROM ax_authors WHERE id = :a", p.clone()),
        c.soft("their newest papers", "SELECT id, title, abstract FROM ax_papers WHERE people = :a AND nauthors <= 50 ORDER BY created DESC LIMIT 8", p.clone()),
        c.soft("their most cited papers of the last six years", "SELECT id, title, abstract FROM ax_papers WHERE people = :a AND nauthors <= 50 AND created >= now() - INTERVAL '2190 days' ORDER BY cites DESC LIMIT 8", p.clone()),
        c.soft("everyone they wrote with", "SELECT people, nauthors FROM ax_papers WHERE people = :a LIMIT 10000", p.clone()),
    );
    let row = row.into_iter().next()?;
    // Their work: what they write now and what they are known for lately.
    let mut seen = HashSet::new();
    let text: String = newest.iter().chain(cited.iter()).filter(|r| seen.insert(r.s("id"))).map(|r| format!("{}. {}", r.s("title"), r.s("abstract"))).collect::<Vec<_>>().join("\n");
    if text.is_empty() {
        return Some(json!({"person": row, "people": [], "note": "no papers of theirs to compare with"}));
    }
    let strs = |r: &Row, k: &str| r.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    let co_all: HashSet<String> = all_mine.iter().flat_map(|r| strs(r, "people")).filter(|x| x != me).collect();
    let co: HashSet<String> = all_mine.iter().filter(|r| r.i("nauthors") <= 50).flat_map(|r| strs(r, "people")).filter(|x| x != me).collect();
    let nearest = c.soft("the papers nearest their work (the last four years)", "SELECT id, people FROM ax_papers WHERE created >= now() - INTERVAL '1460 days' AND nauthors <= 30 ORDER BY similarity(emb, :v) DESC LIMIT 500", json!({"v": {"embed": text, "field": "emb", "table": "ax_papers"}})).await;
    let list: Vec<(String, Vec<String>)> = nearest.iter().map(|r| (r.s("id"), strs(r, "people"))).collect();
    let ranked: Vec<(String, f64, Vec<String>)> = by_ranks(&list, me).into_iter().filter(|x| !co_all.contains(&x.0)).take(30).collect();
    if ranked.is_empty() {
        return Some(json!({"person": row, "people": []}));
    }
    let cands: Vec<String> = ranked.iter().map(|x| x.0.clone()).collect();
    let (rows, theirs) = tokio::join!(
        c.soft("the candidates", "SELECT id, name, papers, cites, h, techniques, cats, last FROM ax_authors WHERE id IN (:c)", json!({"c": cands})),
        c.soft("who the candidates wrote with (the last six years)", "SELECT people FROM ax_papers WHERE people IN (:c) AND nauthors <= 50 AND created >= now() - INTERVAL '2190 days' LIMIT 10000", json!({"c": cands})),
    );
    let by: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    let cand_set: HashSet<&str> = cands.iter().map(|s| s.as_str()).collect();
    let mut co_of: HashMap<String, HashSet<String>> = HashMap::new();
    for r in &theirs {
        let ps = strs(r, "people");
        for a in ps.iter().filter(|a| cand_set.contains(a.as_str())) {
            co_of.entry(a.clone()).or_default().extend(ps.iter().filter(|x| *x != a).cloned());
        }
    }
    // Not two steps away either: no one they both wrote with. Closeness
    // counts most; a record of cited work a little.
    let weight = |x: &(String, f64, Vec<String>)| x.1 * (1.0 + (1.0 + by.get(&x.0).map(|r| r.f("h")).unwrap_or(0.0)).ln() / 3.0);
    let mut far: Vec<&(String, f64, Vec<String>)> = ranked.iter().filter(|x| co_of.get(&x.0).map_or(true, |t| t.is_disjoint(&co))).collect();
    far.sort_by(|a, b| weight(b).partial_cmp(&weight(a)).unwrap_or(std::cmp::Ordering::Equal));
    // Three steps: one of theirs works repeatedly with one of the candidate's.
    let mut co_list: Vec<String> = co.iter().cloned().collect();
    co_list.sort();
    co_list.truncate(800);
    let their_co: Vec<String> = far.iter().flat_map(|x| co_of.get(&x.0).into_iter().flatten().cloned()).collect::<HashSet<_>>().into_iter().take(4000).collect();
    let (e1, e2) = if co_list.is_empty() || their_co.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        tokio::join!(
            c.gsoft("their coauthors' repeated coauthors (one way)", "SELECT src, dst FROM ax_coauthors WHERE src IN (:a) AND dst IN (:b) LIMIT 10000", json!({"a": co_list, "b": their_co})),
            c.gsoft("their coauthors' repeated coauthors (the other way)", "SELECT src, dst FROM ax_coauthors WHERE dst IN (:a) AND src IN (:b) LIMIT 10000", json!({"a": co_list, "b": their_co})),
        )
    };
    // y (a candidate's coauthor) -> x (the person's coauthors who write with y).
    let mut bridge: HashMap<String, Vec<String>> = HashMap::new();
    for r in e1.iter() {
        bridge.entry(r.s("dst")).or_default().push(r.s("src"));
    }
    for r in e2.iter() {
        bridge.entry(r.s("src")).or_default().push(r.s("dst"));
    }
    let mine_t: HashSet<String> = strs(&row, "techniques").into_iter().collect();
    let dict = crate::concepts::dict();
    let tname = |k: &str| dict.by_slug(k).map(|c| c.name.clone()).unwrap_or(k.to_string());
    let mut out = Vec::new();
    let mut want_papers: Vec<String> = Vec::new();
    let mut names_needed: HashSet<String> = HashSet::new();
    for (b, score, papers) in far.iter().map(|x| (&x.0, x.1, &x.2)) {
        let Some(r) = by.get(b) else { continue };
        // Active in the last two years, with five papers or more.
        if r.i("papers") < 5 || r.s("last").as_str() < (chrono::Utc::now() - chrono::Duration::days(730)).to_rfc3339().as_str() {
            continue;
        }
        // Every path of three steps; the clearest is picked once their rows are read.
        let mut paths: Vec<(String, String)> = co_of.get(b).into_iter().flatten().filter_map(|y| bridge.get(y).map(|xs| xs.iter().map(move |x| (x.clone(), y.clone())))).flatten().collect();
        paths.sort();
        paths.truncate(60);
        for (x, y) in &paths {
            names_needed.insert(x.clone());
            names_needed.insert(y.clone());
        }
        let bring: Vec<Value> = strs(r, "techniques").into_iter().filter(|t| !mine_t.contains(t)).take(3).map(|t| json!({"id": t, "name": tname(&t)})).collect();
        want_papers.extend(papers.iter().take(2).cloned());
        out.push(json!({
            "id": b, "name": r.s("name"), "papers": r.i("papers"), "cites": r.i("cites"), "h": r.i("h"), "cats": r.get("cats"),
            "score": (score * 1000.0).round() / 1000.0, "steps": 4,
            "via": paths.iter().map(|(x, y)| json!([x, y])).collect::<Vec<_>>(), "bring": bring, "close": papers.iter().take(2).collect::<Vec<_>>(),
        }));
        if out.len() >= 12 {
            break;
        }
    }
    let names_list: Vec<String> = names_needed.into_iter().collect();
    let (paper_rows, name_rows) = tokio::join!(
        async { if want_papers.is_empty() { Vec::new() } else { c.soft("the papers that make them close", &format!("SELECT {COLS} FROM ax_papers WHERE id IN (:ids)"), json!({"ids": want_papers})).await } },
        crate::search::names(c, &names_list),
    );
    let pby: HashMap<String, Row> = paper_rows.into_iter().map(|r| (r.s("id"), r)).collect();
    for o in out.iter_mut() {
        let close: Vec<Value> = o["close"].as_array().into_iter().flatten().filter_map(|i| i.as_str().and_then(|i| pby.get(i)).map(|r| json!(r))).collect();
        o["close"] = json!(close);
        // The clearest path: through the people with the fewest papers, since
        // a name shared by many people (one key here) gathers thousands.
        // More than 600 papers is a name many people share, not one person.
        let papers_of = |k: &str| name_rows.get(k).map(|r| r.i("papers")).filter(|n| *n <= 600).unwrap_or(i64::MAX / 4);
        let best = o["via"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).filter(|(x, y)| papers_of(x) < i64::MAX / 4 && papers_of(y) < i64::MAX / 4).min_by_key(|(x, y)| papers_of(x) + papers_of(y));
        if best.is_some() {
            o["steps"] = json!(3);
        }
        o["via"] = match best {
            Some((x, y)) => json!([x.as_str(), y.as_str()].iter().map(|k| json!({"id": k, "name": name_rows.get(*k).map(|r| r.s("name")).unwrap_or(k.to_string()), "papers": papers_of(k)})).collect::<Vec<_>>()),
            None => Value::Null,
        };
    }
    let mine: Vec<Value> = strs(&row, "techniques").into_iter().take(10).map(|t| json!({"id": t, "name": tname(&t)})).collect();
    Some(json!({
        "person": row, "mine": mine, "people": out,
        "read": {"papers": nearest.len(), "candidates": ranked.len(), "coauthors": co_all.len()},
    }))
}

/// Reviewers for a paper: people who know its subject (their papers among
/// the 500 nearest to its title and abstract in the last six years), with
/// eight papers or more and one in the last three years, none of its
/// authors and no one who wrote with one of them in the last four years.
/// Those left out for that are named with the author they wrote with. A
/// name with more than 600 papers is many people sharing it, not one to ask.
pub async fn reviewers(c: &Ctx<'_>, title: &str, abstract_: &str, group: &str, authors: &[String], skip: Option<&str>) -> Value {
    let strs = |r: &Row, k: &str| r.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    let text = format!("{title}. {abstract_}");
    let (nearest, theirs) = tokio::join!(
        c.soft("the papers nearest it (the last six years)", "SELECT id, people FROM ax_papers WHERE created >= now() - INTERVAL '2190 days' AND nauthors <= 30 ORDER BY similarity(emb, :v) DESC LIMIT 500", json!({"v": {"embed": text, "field": "emb", "table": "ax_papers"}})),
        async {
            if authors.is_empty() { Vec::new() } else {
                c.soft("who its authors wrote with (the last four years)", "SELECT people, nauthors FROM ax_papers WHERE people IN (:a) AND created >= now() - INTERVAL '1460 days' LIMIT 10000", json!({"a": authors})).await
            }
        },
    );
    // Conflicts: each with the author they wrote with.
    let mine: HashSet<&str> = authors.iter().map(|a| a.as_str()).collect();
    let mut conflict: HashMap<String, String> = authors.iter().map(|a| (a.clone(), a.clone())).collect();
    for r in &theirs {
        let ps = strs(r, "people");
        if let Some(a) = ps.iter().find(|p| mine.contains(p.as_str())) {
            for p in &ps {
                conflict.entry(p.clone()).or_insert(a.clone());
            }
        }
    }
    let list: Vec<(String, Vec<String>)> = nearest.iter().filter(|r| Some(r.s("id").as_str()) != skip).map(|r| (r.s("id"), strs(r, "people"))).collect();
    let all = by_ranks(&list, "");
    let left: Vec<(String, String)> = all.iter().filter(|x| !mine.contains(x.0.as_str())).filter_map(|x| conflict.get(&x.0).map(|a| (x.0.clone(), a.clone()))).take(6).collect();
    let ranked: Vec<&(String, f64, Vec<String>)> = all.iter().filter(|x| !conflict.contains_key(&x.0)).take(40).collect();
    let cands: Vec<String> = ranked.iter().map(|x| x.0.clone()).collect();
    let mut keys: Vec<String> = cands.clone();
    keys.extend(left.iter().flat_map(|(p, a)| [p.clone(), a.clone()]));
    let rows = if keys.is_empty() { Vec::new() } else { c.soft("the people", "SELECT id, name, papers, cites, h, techniques, cats, last FROM ax_authors WHERE id IN (:c)", json!({"c": keys})).await };
    let by: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    let recent = (chrono::Utc::now() - chrono::Duration::days(1095)).to_rfc3339();
    let weight = |x: &(String, f64, Vec<String>)| x.1 * (1.0 + (1.0 + by.get(&x.0).map(|r| r.f("h")).unwrap_or(0.0)).ln() / 3.0);
    let mut good: Vec<&(String, f64, Vec<String>)> = ranked.into_iter().filter(|x| by.get(&x.0).is_some_and(|r| (8..=600).contains(&r.i("papers")) && r.s("last").as_str() >= recent.as_str())).collect();
    good.sort_by(|a, b| weight(b).partial_cmp(&weight(a)).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    good.truncate(12);
    let (topic_t, topic_d) = crate::concepts::dict().tag(title, abstract_, group);
    let subject: HashSet<String> = topic_t.iter().chain(topic_d.iter()).cloned().collect();
    let dict = crate::concepts::dict();
    let tname = |k: &str| dict.by_slug(k).map(|c| c.name.clone()).unwrap_or(k.to_string());
    let want: Vec<String> = good.iter().flat_map(|x| x.2.iter().take(2).cloned()).collect::<HashSet<_>>().into_iter().collect();
    let prow = if want.is_empty() { Vec::new() } else { c.soft("their nearest papers", &format!("SELECT {COLS} FROM ax_papers WHERE id IN (:ids)"), json!({"ids": want})).await };
    let pby: HashMap<String, Row> = prow.into_iter().map(|r| (r.s("id"), r)).collect();
    let people: Vec<Value> = good.iter().map(|x| {
        let r = &by[&x.0];
        let knows: Vec<Value> = strs(r, "techniques").into_iter().filter(|t| subject.contains(t)).take(4).map(|t| json!({"id": t, "name": tname(&t)})).collect();
        json!({
            "id": x.0, "name": r.s("name"), "papers": r.i("papers"), "cites": r.i("cites"), "h": r.i("h"), "cats": r.get("cats"), "last": r.s("last"),
            "near": x.2.len(), "knows": knows,
            "close": x.2.iter().take(2).filter_map(|i| pby.get(i)).collect::<Vec<_>>(),
        })
    }).collect();
    let nm = |k: &str| by.get(k).map(|r| r.s("name")).unwrap_or(k.to_string());
    let left: Vec<Value> = left.iter().map(|(p, a)| json!({"id": p, "name": nm(p), "with": {"id": a, "name": nm(a)}, "author": p == a})).collect();
    json!({
        "people": people, "left_out": left,
        "subject": subject.iter().take(8).map(|t| json!({"id": t, "name": tname(t)})).collect::<Vec<_>>(),
        "read": {"papers": list.len(), "conflicts": conflict.len()},
    })
}

/// Rising people: those whose papers of the last `days` drew the most
/// citations, among people whose first paper on arXiv is at most five years
/// old (early in their careers), with two cited papers of the span or more
/// and between three and 600 papers in all, in an
/// area or all of arXiv. Papers of more than 20 authors are left out, so
/// the credit is for work a person had a real share in.
pub async fn rising(c: &Ctx<'_>, area: &str, days: i64) -> Value {
    let strs = |r: &Row, k: &str| r.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    let days = days.clamp(30, 1460);
    let filter = if area.is_empty() { String::new() } else { "AND areas = :area ".to_string() };
    let sql = format!("SELECT id, people, cites FROM ax_papers WHERE created >= now() - INTERVAL '{days} days' AND nauthors <= 20 AND cites > 0 {filter}ORDER BY cites DESC LIMIT 5000");
    let papers = c.soft("the most cited papers of the span", &sql, json!({"area": area})).await;
    let mut by: HashMap<String, (i64, i64, String)> = HashMap::new();
    for r in &papers {
        for p in strs(r, "people") {
            let e = by.entry(p).or_insert((0, 0, r.s("id")));
            e.0 += r.i("cites");
            e.1 += 1;
        }
    }
    let mut top: Vec<(String, (i64, i64, String))> = by.into_iter().collect();
    top.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    top.truncate(400);
    let keys: Vec<String> = top.iter().map(|x| x.0.clone()).collect();
    let rows = if keys.is_empty() { Vec::new() } else { c.soft("who they are", "SELECT id, name, papers, cites, h, first, last, cats, techniques FROM ax_authors WHERE id IN (:k)", json!({"k": keys})).await };
    let rows: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    let since = (chrono::Utc::now() - chrono::Duration::days(5 * 365)).to_rfc3339();
    // Two cited papers of the span at least, and no more than two people
    // for whom one paper is the best: a list of people, not of one paper's authors.
    let mut per_best: HashMap<&str, usize> = HashMap::new();
    let picked: Vec<&(String, (i64, i64, String))> = top
        .iter()
        .filter(|x| x.1 .1 >= 2 && rows.get(&x.0).is_some_and(|r| r.s("first").as_str() >= since.as_str() && (3..=600).contains(&r.i("papers"))))
        .filter(|x| {
            let n = per_best.entry(x.1 .2.as_str()).or_default();
            *n += 1;
            *n <= 2
        })
        .take(24)
        .collect();
    let want: Vec<String> = picked.iter().map(|x| x.1 .2.clone()).collect();
    let prow = if want.is_empty() { Vec::new() } else { c.soft("their most cited paper of the span", &format!("SELECT {COLS} FROM ax_papers WHERE id IN (:ids)"), json!({"ids": want})).await };
    let pby: HashMap<String, Row> = prow.into_iter().map(|r| (r.s("id"), r)).collect();
    let dict = crate::concepts::dict();
    let people: Vec<Value> = picked.iter().map(|(id, (cites, n, best))| {
        let r = &rows[id];
        json!({
            "id": id, "name": r.s("name"), "papers": r.i("papers"), "h": r.i("h"), "first": r.s("first"), "cats": r.get("cats"),
            "span_cites": cites, "span_papers": n, "best": pby.get(best),
            "techniques": strs(r, "techniques").into_iter().take(2).map(|t| json!({"id": t, "name": dict.by_slug(&t).map(|c| c.name.clone()).unwrap_or(t.clone())})).collect::<Vec<_>>(),
        })
    }).collect();
    json!({"people": people, "read": {"papers": papers.len(), "people": top.len()}, "days": days, "area": area})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn people_score_by_their_papers_ranks_and_need_two() {
        let papers = vec![
            ("p1".to_string(), s(&["me", "x"])),
            ("p2".to_string(), s(&["b", "c"])),
            ("p3".to_string(), s(&["b"])),
            ("p4".to_string(), s(&["c", "d"])),
            ("p5".to_string(), s(&["e"])),
        ];
        let got = by_ranks(&papers, "me");
        let ids: Vec<&str> = got.iter().map(|x| x.0.as_str()).collect();
        // x is only on the person's own paper; d and e have one paper each.
        assert_eq!(ids, vec!["b", "c"]);
        assert_eq!(got[0].2, s(&["p2", "p3"]));
    }
}
