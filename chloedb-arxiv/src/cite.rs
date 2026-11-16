//! Missing citations: a draft's abstract and references in, the papers it
//! should probably cite out.
//!
//! References are read from the pasted text: arXiv ids, DOIs (matched to
//! the papers' `doi`) and, for a reference written out, its title (the
//! best title match by BM25 whose words the line holds). Two lists are
//! fused by rank: what other papers cite together with those references
//! (the recommender's pair counts, normalised by both papers' counts and
//! summed over the references), and what is near the references by
//! meaning (each one's 50 nearest). The abstract's nearest join only when
//! there are fewer than three references. A test on 15 papers of October
//! 2026 (after the citation data, so their own lists are not in the
//! counts) with a fifth of the references they resolve hidden found 14 of
//! the 72 hidden in the top 30 this way, against 7 for raw pair counts
//! and the abstract, 3 for the abstract alone, and none for PageRank over
//! the citations. What is already cited is left out, and each suggestion
//! says why it is there.

use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::schema::RECOMMENDER;
use crate::search::{Ctx, COLS};
use crate::strata::{Row, RowExt};

/// What the pasted text holds.
#[derive(Default, Debug)]
pub struct Draft {
    pub ids: Vec<String>,
    pub dois: Vec<String>,
    /// Reference lines with neither an id nor a DOI.
    pub lines: Vec<String>,
    /// The rest: the abstract or draft text.
    pub prose: String,
}

pub fn read(text: &str) -> Draft {
    let new_id = Regex::new(r"(?i)(?:arxiv[:\s/]*|abs/|pdf/)?\b(\d{4}\.\d{4,5})(?:v\d+)?\b").unwrap();
    let old_id = Regex::new(r"\b([a-z][a-z\-]+(?:\.[A-Z]{2})?/\d{7})(?:v\d+)?\b").unwrap();
    let doi = Regex::new(r"\b(10\.\d{4,9}/[^\s,;<>]+[^\s,;<>.)\]])").unwrap();
    let numbered = Regex::new(r"^\s*(\[\d+\]|\d+\.\s|•|-\s)").unwrap();
    let year = Regex::new(r"\b(19|20)\d\d[a-z]?\b").unwrap();
    let mut d = Draft::default();
    let mut prose: Vec<&str> = Vec::new();
    let mut in_refs = false;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let low = l.to_lowercase();
        if matches!(low.trim_end_matches(':'), "references" | "bibliography" | "works cited" | "related work references") {
            in_refs = true;
            continue;
        }
        let mut found = false;
        for c in new_id.captures_iter(l) {
            d.ids.push(c[1].to_string());
            found = true;
        }
        for c in old_id.captures_iter(l) {
            d.ids.push(c[1].to_string());
            found = true;
        }
        for c in doi.captures_iter(l) {
            // Kept as written and in lower case: the papers' DOIs keep their case.
            d.dois.push(c[1].to_string());
            d.dois.push(c[1].to_lowercase());
            found = true;
        }
        let words = l.split_whitespace().count();
        let looks_ref = in_refs || numbered.is_match(l) || (words < 80 && (low.contains("et al") || (year.is_match(l) && l.contains('.') && words < 60)));
        if found {
            continue;
        }
        if looks_ref && words >= 4 {
            d.lines.push(l.to_string());
        } else {
            prose.push(l);
        }
    }
    d.prose = prose.join(" ");
    d.ids.sort();
    d.ids.dedup();
    d.dois.sort();
    d.dois.dedup();
    d.lines.truncate(60);
    d
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(String::from).collect()
}

/// A written-out reference's paper: the best title match whose words the
/// line holds (85% of them or more).
async fn by_title(c: &Ctx<'_>, line: &str) -> Option<Row> {
    // A reference line holds authors and a venue too: a quarter of its words is enough to be a candidate.
    let rows = c.soft("a reference by its title", &format!("SELECT {COLS} FROM ax_papers WHERE MATCH(title, :q, min => '25%') ORDER BY bm25(title, :q, min => '25%') DESC LIMIT 3"), json!({"q": line})).await;
    let have: HashSet<String> = words(line).into_iter().collect();
    rows.into_iter().find(|r| {
        let t = words(&r.s("title"));
        t.len() >= 2 && t.iter().filter(|w| have.contains(*w)).count() * 100 >= t.len() * 85
    })
}

pub async fn suggest(c: &Ctx<'_>, text: &str, before: Option<&str>, only: Option<&str>, k: usize) -> Value {
    let use_list = |l: &str| only.is_none_or(|o| o == "all" || o == l);
    let d = read(text);
    // The references, as papers here.
    let (by_id, by_doi) = tokio::join!(
        crate::pages::papers_by_id(c, "the references, by arXiv id", &d.ids, COLS),
        async {
            if d.dois.is_empty() {
                Vec::new()
            } else {
                c.soft("the references, by DOI", &format!("SELECT {COLS} FROM ax_papers WHERE doi IN (:d)"), json!({"d": d.dois})).await
            }
        },
    );
    let mut titled: Vec<Row> = Vec::new();
    for chunk in d.lines.chunks(10) {
        let got = futures_util::future::join_all(chunk.iter().map(|l| by_title(c, l))).await;
        titled.extend(got.into_iter().flatten());
    }
    let mut refs: Vec<Row> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for r in by_id.into_iter().chain(by_doi).chain(titled) {
        if seen.insert(r.s("id")) {
            refs.push(r);
        }
    }
    let seeds: Vec<String> = refs.iter().map(|r| r.s("id")).collect();
    let prose = if d.prose.split_whitespace().count() >= 8 { d.prose.clone() } else { refs.iter().map(|r| r.s("title")).collect::<Vec<_>>().join(". ") };
    if seeds.is_empty() && prose.split_whitespace().count() < 8 {
        return json!({"refs": [], "found": {"ids": d.ids.len(), "dois": d.dois.len(), "lines": d.lines.len()}, "papers": [], "note": "Paste an abstract (a few sentences) or references with arXiv ids, DOIs or titles."});
    }
    // Cited with the references: the recommender's pair counts, each
    // divided by the square root of both papers' counts (so a paper cited
    // everywhere does not win by size), summed over the references.
    let counts = format!("{RECOMMENDER}_counts");
    let pairs = if seeds.is_empty() {
        Vec::new()
    } else {
        c.gsoft("cited together with the references (the recommender's pair counts)", &format!("SELECT src, dst, co FROM {counts} WHERE kind = 'pair' AND src IN (:s) ORDER BY co DESC LIMIT 10000"), json!({"s": seeds})).await
    };
    let mut with_n: HashMap<String, (f64, i64)> = HashMap::new();
    let pair_ids: Vec<String> = pairs.iter().map(|r| r.s("dst")).chain(seeds.iter().cloned()).collect::<HashSet<_>>().into_iter().collect();
    let mut items: HashMap<String, f64> = HashMap::new();
    for chunk in pair_ids.chunks(5000) {
        for r in c.gsoft("how often each is cited (the recommender's item counts)", &format!("SELECT src, n FROM {counts} WHERE kind = 'item' AND src IN (:s) LIMIT 10000"), json!({"s": chunk})).await {
            items.insert(r.s("src"), r.f("n").max(1.0));
        }
    }
    for r in &pairs {
        let (g, h) = (r.s("src"), r.s("dst"));
        if seen.contains(&h) {
            continue;
        }
        let e = with_n.entry(h.clone()).or_insert((0.0, 0));
        e.0 += r.f("co") / (items.get(&g).copied().unwrap_or(1.0) * items.get(&h).copied().unwrap_or(1.0)).sqrt();
        e.1 += 1;
    }
    let mut cited_with: Vec<(String, f64)> = with_n.iter().map(|(k, v)| (k.clone(), v.0)).collect();
    cited_with.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // Near the references by meaning: each reference's 50 nearest, a
    // paper scoring by how many of them it is near and how near.
    let mut near_refs: HashMap<String, (f64, i64)> = HashMap::new();
    let probe: Vec<String> = seeds.iter().take(40).cloned().collect();
    for chunk in probe.chunks(10) {
        let got = futures_util::future::join_all(chunk.iter().map(|g| c.soft("the nearest to a reference by meaning", "SELECT id FROM ax_papers WHERE id != :g ORDER BY similarity(emb, :v) DESC LIMIT 50", json!({"g": g, "v": {"vector_of": g, "table": "ax_papers", "field": "emb"}})))).await;
        for rows in got {
            for (i, r) in rows.iter().enumerate() {
                let k = r.s("id");
                if !seen.contains(&k) {
                    let e = near_refs.entry(k).or_insert((0.0, 0));
                    e.0 += 1.0 / (10.0 + i as f64);
                    e.1 += 1;
                }
            }
        }
    }
    let mut near_list: Vec<(String, f64)> = near_refs.iter().map(|(k, v)| (k.clone(), v.0)).collect();
    near_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // Near the abstract, when there are few references to go on.
    let mut p = Map::new();
    p.insert("v".into(), json!({"embed": prose, "field": "emb", "table": "ax_papers"}));
    let mut wh: Vec<String> = Vec::new();
    if !seeds.is_empty() {
        p.insert("skip".into(), json!(seeds));
        wh.push("id NOT IN (:skip)".into());
    }
    if let Some(b) = before {
        p.insert("before".into(), json!(b));
        wh.push("created < :before".into());
    }
    let wh = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
    let near_abs: Vec<String> = c.soft("the closest to the abstract by meaning", &format!("SELECT id FROM ax_papers {wh} ORDER BY similarity(emb, :v) DESC LIMIT 100"), Value::Object(p.clone())).await.iter().map(|r| r.s("id")).collect();
    // Fused by rank (reciprocal rank fusion, as rrf() does), the lists the
    // test on recent papers' real references chose: cited with and near
    // the references; the abstract only with fewer than three references.
    let mut lists: Vec<Vec<String>> = Vec::new();
    if use_list("cited_with") {
        lists.push(cited_with.iter().map(|x| x.0.clone()).collect());
    }
    if use_list("near_refs") {
        lists.push(near_list.iter().map(|x| x.0.clone()).collect());
    }
    if only == Some("meaning") || seeds.len() < 3 || lists.iter().all(|l| l.is_empty()) {
        lists.push(near_abs.clone());
    }
    let mut fused: HashMap<String, f64> = HashMap::new();
    for l in &lists {
        for (i, k) in l.iter().enumerate() {
            *fused.entry(k.clone()).or_default() += 1.0 / (60.0 + i as f64);
        }
    }
    let mut order: Vec<(String, f64)> = fused.into_iter().collect();
    order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let cand: Vec<String> = order.iter().take(k * 3).map(|x| x.0.clone()).collect();
    let mut pr = Map::new();
    pr.insert("ids".into(), json!(cand));
    let mut cw = vec!["id IN (:ids)".to_string()];
    if let Some(b) = before {
        pr.insert("before".into(), json!(b));
        cw.push("created < :before".into());
    }
    let rows = if cand.is_empty() { Vec::new() } else { c.soft("the suggestions' rows", &format!("SELECT {COLS} FROM ax_papers WHERE {}", cw.join(" AND ")), Value::Object(pr)).await };
    let by_id: HashMap<String, &Row> = rows.iter().map(|r| (r.s("id"), r)).collect();
    let abs_pos: HashMap<&String, usize> = near_abs.iter().enumerate().map(|(i, x)| (x, i)).collect();
    let papers: Vec<Value> = cand.iter().filter_map(|id| by_id.get(id).map(|r| (id, *r))).take(k).map(|(id, r)| {
        let mut why: Vec<String> = Vec::new();
        if let Some((_, n)) = with_n.get(id) {
            why.push(if *n > 1 { format!("cited with {n} of your references") } else { "cited with one of your references".into() });
        }
        if let Some((_, n)) = near_refs.get(id) {
            why.push(if *n > 1 { format!("close in meaning to {n} of them") } else { "close in meaning to one of them".into() });
        }
        if abs_pos.get(id).is_some_and(|i| *i < 30) {
            why.push("close to your abstract".into());
        }
        let mut v = json!(r);
        v["why"] = json!(why);
        v
    }).collect();
    json!({
        "refs": refs, "found": {"ids": d.ids.len(), "dois": d.dois.len(), "lines": d.lines.len(), "prose_words": d.prose.split_whitespace().count()},
        "papers": papers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_draft() {
        let d = read("We study chain of thought prompting for small models and find it helps when distilled.\n\nReferences\n[1] J. Wei et al. Chain-of-thought prompting elicits reasoning in large language models. NeurIPS 2022.\n[2] arXiv:2203.11171\n[3] https://arxiv.org/abs/2005.11401v4\n[4] doi:10.1145/3442188.3445922.\nhep-th/9711200");
        assert_eq!(d.ids, vec!["2005.11401", "2203.11171", "hep-th/9711200"]);
        assert_eq!(d.dois, vec!["10.1145/3442188.3445922"]);
        assert_eq!(d.lines.len(), 1);
        assert!(d.prose.starts_with("We study chain of thought"));
    }
}
