//! One link across Hacker News: every time it was submitted, every comment
//! elsewhere that cites it, the discussions that point at its discussions,
//! and the people around it, as lists and as a graph. A bare host (or a
//! site, `github.com/rust-lang`) gives the site's most discussed links and
//! the people who submit them.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::search::{like, Ctx, STORY_COLS};
use crate::strata::{Row, RowExt};
use crate::text;

/// What was asked, made a link: a scheme added when it has none.
pub fn parse(input: &str) -> (String, String) {
    let t = input.trim();
    let url = if t.contains("://") { t.to_string() } else { format!("https://{t}") };
    let curl = text::canonical_url(&url);
    (url, curl)
}

/// Whether the canonical link names a whole site (a host, or a host and
/// one part for the hosts of many people's pages) rather than a page.
pub fn is_site(url: &str, curl: &str) -> bool {
    !curl.contains('/') || text::site(url) == curl
}

fn ids_of(rows: &[Row], col: &str) -> Vec<i64> {
    let mut seen = HashSet::new();
    rows.iter().map(|r| r.i(col)).filter(|&i| i > 0 && seen.insert(i)).collect()
}

fn list(ids: &[i64]) -> String {
    ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
}

fn short(t: &str, n: usize) -> String {
    let t = crate::search::short_title(t);
    if t.chars().count() <= n {
        return t.to_string();
    }
    format!("{}…", t.chars().take(n - 1).collect::<String>().trim_end())
}

/// A person's part around the link.
#[derive(Default)]
struct Roles {
    submitted: i64,
    commented: i64,
    cited: i64,
}

pub async fn link(c: &Ctx<'_>, input: &str) -> Value {
    let (url, curl) = parse(input);
    if curl.is_empty() {
        return json!({"error": "not a link"});
    }
    if is_site(&url, &curl) {
        return site(c, &curl).await;
    }
    // The pattern for comments that cite it: the link without its query.
    let core = curl.split('?').next().unwrap_or(&curl).to_string();
    let pat = like(&core);
    let (subs, cited, cited_n) = tokio::join!(
        c.soft("every submission of the link", &format!("SELECT {STORY_COLS}, curl FROM hn_stories WHERE curl = :c ORDER BY created LIMIT 60"), json!({"c": curl})),
        c.within("comments that cite it (substring index)", "SELECT id, author, story, story_title, created, body FROM hn_comments WHERE body ILIKE :p ORDER BY created DESC LIMIT 50", json!({"p": pat}), 3000),
        c.within("how many comments cite it", "SELECT COUNT(*) AS n FROM hn_comments WHERE body ILIKE :p", json!({"p": pat}), 3000),
    );
    let sub_ids = ids_of(&subs, "id");
    let sub_set: HashSet<i64> = sub_ids.iter().copied().collect();
    // Comments in its own discussions that name it are not citations elsewhere.
    let cited: Vec<Row> = cited.into_iter().filter(|r| !sub_set.contains(&r.i("story"))).collect();
    let cite_story_ids = ids_of(&cited, "story");
    let (commenters, links, cite_stories) = tokio::join!(
        async {
            if sub_ids.is_empty() {
                return Vec::new();
            }
            c.within("who talked in its discussions", &format!("SELECT story, author, COUNT(*) AS n FROM hn_comments WHERE story IN ({}) GROUP BY story, author ORDER BY n DESC LIMIT 120", list(&sub_ids)), Value::Null, 3000).await
        },
        async {
            if sub_ids.is_empty() {
                return Vec::new();
            }
            let l = list(&sub_ids);
            c.soft("discussions that point at its discussions", &format!("SELECT src, dst, w FROM hn_links WHERE src IN ({l}) OR dst IN ({l}) LIMIT 80"), Value::Null).await
        },
        async {
            if cite_story_ids.is_empty() {
                return Vec::new();
            }
            c.soft("the discussions where it was cited", &format!("SELECT id, title, url, curl, site, points, comments, created FROM hn_stories WHERE id IN ({})", list(&cite_story_ids[..cite_story_ids.len().min(60)])), Value::Null).await
        },
    );
    // The far ends of the discussion links.
    let far: Vec<i64> = links.iter().flat_map(|r| [r.i("src"), r.i("dst")]).filter(|i| *i > 0 && !sub_set.contains(i)).collect::<HashSet<_>>().into_iter().take(40).collect();
    let far_rows = if far.is_empty() { Vec::new() } else { c.soft("those discussions", &format!("SELECT id, title, url, curl, points, comments, created FROM hn_stories WHERE id IN ({})", list(&far)), Value::Null).await };

    // People: who submitted it, who talked in its discussions, who cited it elsewhere.
    let mut roles: HashMap<String, Roles> = HashMap::new();
    for s in &subs {
        roles.entry(s.s("author")).or_default().submitted += 1;
    }
    for r in &commenters {
        roles.entry(r.s("author")).or_default().commented += r.i("n");
    }
    for r in &cited {
        roles.entry(r.s("author")).or_default().cited += 1;
    }
    roles.remove("");
    let mut people: Vec<(String, Roles)> = roles.into_iter().collect();
    // Several parts first, then the most active.
    people.sort_by_key(|(a, r)| {
        let parts = (r.submitted > 0) as i64 + (r.commented > 0) as i64 + (r.cited > 0) as i64;
        (std::cmp::Reverse(parts), std::cmp::Reverse(r.submitted * 10 + r.cited * 3 + r.commented), a.clone())
    });

    // Related links: the links of the discussions where it was cited.
    let mut related: HashMap<String, (i64, i64, String, i64)> = HashMap::new();
    let by_story: HashMap<i64, &Row> = cite_stories.iter().map(|r| (r.i("id"), r)).collect();
    let mut cites_per_story: HashMap<i64, i64> = HashMap::new();
    for r in &cited {
        *cites_per_story.entry(r.i("story")).or_default() += 1;
    }
    for (sid, n) in &cites_per_story {
        if let Some(s) = by_story.get(sid) {
            let cu = s.s("curl");
            if cu.is_empty() || cu == curl {
                continue;
            }
            let e = related.entry(cu).or_insert((0, 0, s.s("title"), s.i("id")));
            e.0 += n;
            e.1 += s.i("comments");
        }
    }
    let mut related: Vec<(String, (i64, i64, String, i64))> = related.into_iter().collect();
    related.sort_by_key(|(_, v)| (std::cmp::Reverse(v.0), std::cmp::Reverse(v.1)));

    let graph = graph(&curl, &subs, &people, &commenters, &cited, &cite_stories, &links, &far_rows);
    let first = subs.first().map(|s| s.s("created"));
    json!({
        "kind": "link",
        "url": subs.first().map(|s| s.s("url")).filter(|u| !u.is_empty()).unwrap_or(url.clone()),
        "curl": curl, "site": text::site(&url),
        "first": first,
        "totals": {
            "submissions": subs.len(),
            "points": subs.iter().map(|s| s.i("points")).sum::<i64>(),
            "comments": subs.iter().map(|s| s.i("comments")).sum::<i64>(),
            "cited": cited_n.first().map(|r| r.i("n")).unwrap_or(cited.len() as i64),
        },
        "submissions": subs,
        "cited": cited.iter().take(40).map(|r| json!({"id": r.i("id"), "author": r.s("author"), "story": r.i("story"), "story_title": r.s("story_title"), "created": r.s("created"), "text": text::clip(&r.s("body"), 280)})).collect::<Vec<_>>(),
        "people": people.iter().take(30).map(|(a, r)| json!({"author": a, "submitted": r.submitted, "commented": r.commented, "cited": r.cited})).collect::<Vec<_>>(),
        "related": related.iter().take(12).map(|(cu, v)| json!({"curl": cu, "cites": v.0, "comments": v.1, "title": v.2, "story": v.3})).collect::<Vec<_>>(),
        "linked": far_rows.iter().map(|r| json!({"id": r.i("id"), "title": r.s("title"), "curl": r.s("curl"), "points": r.i("points"), "comments": r.i("comments"), "created": r.s("created")})).collect::<Vec<_>>(),
        "graph": graph,
    })
}

/// Nodes and edges for the page to lay out: the link in the middle, its
/// submissions around it, the people on them, the discussions elsewhere
/// that cite it, and the discussions linked to its own.
#[allow(clippy::too_many_arguments)]
fn graph(curl: &str, subs: &[Row], people: &[(String, Roles)], commenters: &[Row], cited: &[Row], cite_stories: &[Row], links: &[Row], far: &[Row]) -> Value {
    let mut nodes: Vec<Value> = vec![json!({"id": "L", "kind": "link", "label": curl, "w": 1.0})];
    let mut edges: Vec<Value> = Vec::new();
    let mut have: HashSet<String> = HashSet::new();
    have.insert("L".into());
    let max_pts = subs.iter().map(|s| s.i("points")).max().unwrap_or(1).max(1) as f64;
    for s in subs.iter().take(24) {
        let id = format!("s{}", s.i("id"));
        let year = s.s("created").get(..4).unwrap_or("").to_string();
        nodes.push(json!({"id": id, "kind": "story", "item": s.i("id"), "label": format!("{year} · {} pts", s.i("points")), "title": short(&s.s("title"), 70), "w": (s.i("points") as f64 / max_pts).sqrt()}));
        edges.push(json!({"a": "L", "b": id, "kind": "submitted"}));
        have.insert(id);
    }
    // People with a part, at most 22: several parts first.
    let keep: Vec<&(String, Roles)> = people.iter().take(22).collect();
    let kept: HashSet<&str> = keep.iter().map(|(a, _)| a.as_str()).collect();
    for (a, r) in &keep {
        let id = format!("u{a}");
        nodes.push(json!({"id": id, "kind": "user", "user": a, "label": a, "w": ((r.submitted * 10 + r.cited * 3 + r.commented) as f64).ln_1p() / 5.0, "roles": {"submitted": r.submitted, "commented": r.commented, "cited": r.cited}}));
        have.insert(id);
    }
    for s in subs.iter().take(24) {
        if kept.contains(s.s("author").as_str()) {
            edges.push(json!({"a": format!("u{}", s.s("author")), "b": format!("s{}", s.i("id")), "kind": "submitted it"}));
        }
    }
    for r in commenters {
        let (a, st) = (r.s("author"), format!("s{}", r.i("story")));
        if kept.contains(a.as_str()) && have.contains(&st) {
            edges.push(json!({"a": format!("u{a}"), "b": st, "kind": "commented", "n": r.i("n")}));
        }
    }
    // Discussions elsewhere that cite it, at most 14, most citations first.
    let mut per: HashMap<i64, i64> = HashMap::new();
    for r in cited {
        *per.entry(r.i("story")).or_default() += 1;
    }
    let titles: HashMap<i64, &Row> = cite_stories.iter().map(|r| (r.i("id"), r)).collect();
    let mut order: Vec<(i64, i64)> = per.into_iter().collect();
    order.sort_by_key(|(id, n)| (std::cmp::Reverse(*n), std::cmp::Reverse(titles.get(id).map(|r| r.i("comments")).unwrap_or(0))));
    for (sid, n) in order.into_iter().take(14) {
        let id = format!("c{sid}");
        let t = titles.get(&sid).map(|r| r.s("title")).unwrap_or_default();
        nodes.push(json!({"id": id, "kind": "cites", "item": sid, "label": short(&t, 46), "title": t, "w": (n as f64).ln_1p() / 3.0}));
        edges.push(json!({"a": id, "b": "L", "kind": "cited", "n": n}));
        have.insert(id.clone());
        for r in cited.iter().filter(|r| r.i("story") == sid) {
            let a = r.s("author");
            if kept.contains(a.as_str()) {
                edges.push(json!({"a": format!("u{a}"), "b": id, "kind": "cited it here"}));
            }
        }
    }
    // Discussions linked to its own (hn_links: a story or comment naming another's item page).
    let far_by: HashMap<i64, &Row> = far.iter().map(|r| (r.i("id"), r)).collect();
    for l in links.iter().take(30) {
        let (src, dst) = (l.i("src"), l.i("dst"));
        let (mine, other, out) = if have.contains(&format!("s{src}")) { (src, dst, true) } else { (dst, src, false) };
        let Some(o) = far_by.get(&other) else { continue };
        let id = format!("d{other}");
        if have.insert(id.clone()) {
            nodes.push(json!({"id": id, "kind": "linked", "item": other, "label": short(&o.s("title"), 46), "title": o.s("title"), "w": 0.3}));
        }
        edges.push(json!({"a": if out { format!("s{mine}") } else { id.clone() }, "b": if out { id } else { format!("s{mine}") }, "kind": "links to"}));
    }
    json!({"nodes": nodes, "edges": edges})
}

/// A whole site: its most discussed links, the links submitted most often,
/// and who submits them.
pub async fn site(c: &Ctx<'_>, site: &str) -> Value {
    let (top, again, people, n) = tokio::join!(
        c.soft("the site's most discussed links", &format!("SELECT {STORY_COLS}, curl FROM hn_stories WHERE site = :s ORDER BY comments DESC LIMIT 40"), json!({"s": site})),
        c.within("the site's links submitted most often", "SELECT curl, COUNT(*) AS n FROM hn_stories WHERE site = :s GROUP BY curl ORDER BY n DESC LIMIT 15", json!({"s": site}), 3000),
        c.within("who submits the site", "SELECT author, COUNT(*) AS n FROM hn_stories WHERE site = :s GROUP BY author ORDER BY n DESC LIMIT 15", json!({"s": site}), 3000),
        c.soft("the site's stories", "SELECT COUNT(*) AS n FROM hn_stories WHERE site = :s", json!({"s": site})),
    );
    json!({
        "kind": "site", "site": site, "curl": site,
        "totals": {"stories": n.first().map(|r| r.i("n")).unwrap_or(0)},
        "top": top,
        "again": again.iter().filter(|r| r.i("n") > 1).map(|r| json!({"curl": r.s("curl"), "n": r.i("n")})).collect::<Vec<_>>(),
        "people": people,
    })
}

/// Links discussed the most, for the page with no link yet.
pub async fn famous(c: &Ctx<'_>) -> Value {
    let rows = c.soft("the most discussed links", "SELECT id, title, url, curl, site, points, comments, created FROM hn_stories WHERE comments > 1500 AND curl != '' ORDER BY comments DESC LIMIT 24", Value::Null).await;
    json!({"kind": "start", "links": rows})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_or_a_site() {
        let (u, c) = parse("http://www.paulgraham.com/ds.html?utm_source=x");
        assert_eq!(c, "paulgraham.com/ds.html");
        assert!(!is_site(&u, &c));
        let (u, c) = parse("paulgraham.com");
        assert_eq!(c, "paulgraham.com");
        assert!(is_site(&u, &c));
        let (u, c) = parse("https://github.com/rust-lang");
        assert!(is_site(&u, &c));
        let (u, c) = parse("https://github.com/rust-lang/rust");
        assert!(!is_site(&u, &c));
    }
}
