//! Labs: the organisations behind the papers, read from OpenAlex's public
//! snapshot (to mid-October 2026) into `ax_lab_papers`, one row a paper
//! with its organisations and the columns counted here. The labs with the
//! most papers of the last twelve months, those growing fastest against the
//! twelve before (in an area, or on one technique: who is moving into
//! it), and a lab's own page: papers a year, what it is known for and what
//! it is moving into, who writes its papers, the labs it writes with.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::search::{Ctx, COLS};
use crate::strata::{Row, RowExt};

fn strs(r: &Row, k: &str) -> Vec<String> {
    r.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

async fn named(c: &Ctx<'_>, ids: &[String]) -> HashMap<String, Row> {
    if ids.is_empty() {
        return HashMap::new();
    }
    c.soft("the labs' names", "SELECT id, name, country, kind, papers, recent, before FROM ax_labs WHERE id IN (:k)", json!({"k": ids})).await.into_iter().map(|r| (r.s("id"), r)).collect()
}

/// Growth of a count against the one before, steadied for small counts.
pub fn growth(recent: i64, before: i64) -> f64 {
    (recent as f64 + 1.0) / (before as f64 + 3.0)
}

/// Growth of a lab's share of the papers with an organisation: OpenAlex
/// names organisations for fewer of the newest papers (it catches up
/// months later), so counts alone would show every lab shrinking.
pub fn share_growth(recent: i64, before: i64, all_recent: i64, all_before: i64) -> f64 {
    growth(recent, before) * (all_before.max(1) as f64 / all_recent.max(1) as f64)
}

const COUNTS: &str = "SELECT COUNT(*) AS n FROM ax_lab_papers WHERE created >= now() - INTERVAL '365 days'";
const COUNTS_BEFORE: &str = "SELECT COUNT(*) AS n FROM ax_lab_papers WHERE created >= now() - INTERVAL '730 days' AND created < now() - INTERVAL '365 days'";

pub async fn index(c: &Ctx<'_>, area: &str, tech: &str) -> Value {
    let mut f = String::new();
    if !area.is_empty() {
        f.push_str(" AND areas = :area");
    }
    if !tech.is_empty() {
        f.push_str(" AND techniques = :t");
    }
    let p = json!({"area": area, "t": tech});
    let (recent, before, all_r, all_b) = tokio::join!(
        c.soft("labs by papers of the last twelve months", &format!("SELECT orgs, COUNT(*) AS n FROM ax_lab_papers WHERE created >= now() - INTERVAL '365 days'{f} GROUP BY orgs ORDER BY n DESC LIMIT 3000"), p.clone()),
        c.soft("labs by papers of the twelve months before", &format!("SELECT orgs, COUNT(*) AS n FROM ax_lab_papers WHERE created >= now() - INTERVAL '730 days' AND created < now() - INTERVAL '365 days'{f} GROUP BY orgs ORDER BY n DESC LIMIT 3000"), p.clone()),
        c.soft("papers with an organisation, last twelve months", &format!("{COUNTS}{f}"), p.clone()),
        c.soft("papers with an organisation, the twelve before", &format!("{COUNTS_BEFORE}{f}"), p.clone()),
    );
    let (ar, ab) = (all_r.first().map(|r| r.i("n")).unwrap_or(1), all_b.first().map(|r| r.i("n")).unwrap_or(1));
    let growth = |r: i64, b: i64| share_growth(r, b, ar, ab);
    let b: HashMap<String, i64> = before.iter().map(|r| (r.s("orgs"), r.i("n"))).collect();
    let rows: Vec<(String, i64, i64)> = recent.iter().map(|r| (r.s("orgs"), r.i("n"), b.get(&r.s("orgs")).copied().unwrap_or(0))).filter(|x| !x.0.is_empty()).collect();
    // Over all of a field, a lab with no papers the year before is most
    // often a new record of OpenAlex's matching, not a new lab: twenty then.
    let (floor, then) = if tech.is_empty() { (40, 20) } else { (8, 0) };
    let mut moving: Vec<&(String, i64, i64)> = rows.iter().filter(|x| x.1 >= floor && x.2 >= then).collect();
    moving.sort_by(|a, b| growth(b.1, b.2).partial_cmp(&growth(a.1, a.2)).unwrap_or(std::cmp::Ordering::Equal));
    let top: Vec<&(String, i64, i64)> = rows.iter().take(25).collect();
    let moving: Vec<&(String, i64, i64)> = moving.into_iter().filter(|x| x.1 > x.2).take(150).collect();
    let ids: Vec<String> = top.iter().chain(moving.iter()).map(|x| x.0.clone()).collect::<HashSet<_>>().into_iter().collect();
    let names = named(c, &ids).await;
    // OpenAlex re-files papers between records of one place (a press, a
    // department, a new record for an old lab), which looks like sudden
    // growth: over all of a field, only labs of 300 papers or more that
    // are not a department.
    let moving: Vec<&(String, i64, i64)> = moving
        .into_iter()
        .filter(|x| {
            !tech.is_empty()
                || names.get(&x.0).is_some_and(|r| {
                    let n = r.s("name").to_lowercase();
                    r.i("papers") >= 300 && !n.starts_with("department") && !n.contains(" press") && !["funder", "archive", "other"].contains(&r.s("kind").as_str())
                })
        })
        .take(25)
        .collect();
    let row = |x: &&(String, i64, i64)| {
        let r = names.get(&x.0);
        json!({"id": x.0, "name": r.map(|r| r.s("name")).unwrap_or(x.0.clone()), "country": r.map(|r| r.s("country")), "kind": r.map(|r| r.s("kind")), "recent": x.1, "before": x.2, "growth": (growth(x.1, x.2) * 100.0).round() / 100.0,
            "change": if x.2 > 0 { Some((x.1 as f64 / ar.max(1) as f64) / (x.2 as f64 / ab.max(1) as f64)) } else { None }})
    };
    json!({
        "area": area, "technique": tech, "technique_name": crate::concepts::dict().by_slug(tech).map(|c| c.name.clone()).unwrap_or(tech.to_string()),
        "with_orgs": {"recent": ar, "before": ab},
        "top": top.iter().map(row).collect::<Vec<_>>(),
        "moving": moving.iter().map(row).collect::<Vec<_>>(),
        "labs": rows.len(),
    })
}

pub async fn lab(c: &Ctx<'_>, id: &str) -> Option<Value> {
    let p = json!({"o": id});
    let (row, years, t_now, t_then, people, partners, countries, newest, cited, all_r, all_b) = tokio::join!(
        c.soft("the lab", "SELECT * FROM ax_labs WHERE id = :o", p.clone()),
        c.soft("its papers a year", "SELECT year, COUNT(*) AS n FROM ax_lab_papers WHERE orgs = :o GROUP BY year ORDER BY year", p.clone()),
        c.soft("its techniques (two years)", "SELECT techniques, COUNT(*) AS n FROM ax_lab_papers WHERE orgs = :o AND created >= now() - INTERVAL '730 days' GROUP BY techniques ORDER BY n DESC LIMIT 200", p.clone()),
        c.soft("its techniques (the two years before)", "SELECT techniques, COUNT(*) AS n FROM ax_lab_papers WHERE orgs = :o AND created >= now() - INTERVAL '1460 days' AND created < now() - INTERVAL '730 days' GROUP BY techniques ORDER BY n DESC LIMIT 400", p.clone()),
        c.soft("who writes its papers (three years)", "SELECT people, COUNT(*) AS n FROM ax_lab_papers WHERE orgs = :o AND created >= now() - INTERVAL '1095 days' GROUP BY people ORDER BY n DESC LIMIT 20", p.clone()),
        c.soft("the labs it writes with (three years)", "SELECT orgs, COUNT(*) AS n FROM ax_lab_papers WHERE orgs = :o AND created >= now() - INTERVAL '1095 days' GROUP BY orgs ORDER BY n DESC LIMIT 16", p.clone()),
        c.soft("the countries it writes with (three years)", "SELECT countries, COUNT(*) AS n FROM ax_lab_papers WHERE orgs = :o AND created >= now() - INTERVAL '1095 days' GROUP BY countries ORDER BY n DESC LIMIT 12", p.clone()),
        c.soft("its newest papers", "SELECT id FROM ax_lab_papers WHERE orgs = :o ORDER BY created DESC LIMIT 10", p.clone()),
        c.soft("its most cited papers", "SELECT id FROM ax_lab_papers WHERE orgs = :o ORDER BY cites DESC LIMIT 10", p.clone()),
        c.soft("papers with an organisation, last twelve months", COUNTS, json!({})),
        c.soft("papers with an organisation, the twelve before", COUNTS_BEFORE, json!({})),
    );
    let mut row = row.into_iter().next()?;
    // Its share of the papers with an organisation, against the year before.
    let (ar, ab) = (all_r.first().map(|r| r.i("n")).unwrap_or(1), all_b.first().map(|r| r.i("n")).unwrap_or(1));
    let (lr, lb) = (row.i("recent"), row.i("before"));
    if lb > 0 {
        row.insert("share_change".into(), json!((lr as f64 / ar.max(1) as f64) / (lb as f64 / ab.max(1) as f64)));
    }
    let ids: Vec<String> = newest.iter().chain(cited.iter()).map(|r| r.s("id")).collect();
    let partner_ids: Vec<String> = partners.iter().map(|r| r.s("orgs")).filter(|o| o != id).collect();
    let people_ids: Vec<String> = people.iter().map(|r| r.s("people")).filter(|x| !x.is_empty()).collect();
    let (prows, pnames, labs) = tokio::join!(
        async { if ids.is_empty() { Vec::new() } else { c.soft("the papers", &format!("SELECT {COLS} FROM ax_papers WHERE id IN (:ids)"), json!({"ids": ids})).await } },
        crate::search::names(c, &people_ids),
        named(c, &partner_ids),
    );
    let pby: HashMap<String, Row> = prows.into_iter().map(|r| (r.s("id"), r)).collect();
    let list = |rows: &[Row]| rows.iter().filter_map(|r| pby.get(&r.s("id")).cloned()).collect::<Vec<_>>();
    let dict = crate::concepts::dict();
    let tname = |k: &str| dict.by_slug(k).map(|c| c.name.clone()).unwrap_or(k.to_string());
    let then: HashMap<String, i64> = t_then.iter().map(|r| (r.s("techniques"), r.i("n"))).collect();
    let known: Vec<Value> = t_now.iter().filter(|r| !r.s("techniques").is_empty()).take(10).map(|r| json!({"id": r.s("techniques"), "name": tname(&r.s("techniques")), "n": r.i("n")})).collect();
    let mut moving: Vec<(String, i64, i64)> = t_now.iter().map(|r| (r.s("techniques"), r.i("n"), then.get(&r.s("techniques")).copied().unwrap_or(0))).filter(|x| !x.0.is_empty() && x.1 >= 5 && x.1 > 2 * x.2).collect();
    moving.sort_by(|a, b| growth(b.1, b.2).partial_cmp(&growth(a.1, a.2)).unwrap_or(std::cmp::Ordering::Equal));
    let total = years.iter().map(|r| r.i("n")).sum::<i64>().max(1);
    let _ = strs;
    Some(json!({
        "lab": row,
        "years": years.iter().filter(|r| r.i("year") > 1990).map(|r| json!({"year": r.i("year"), "n": r.i("n")})).collect::<Vec<_>>(),
        "known": known,
        "moving": moving.iter().take(10).map(|(t, n, b)| json!({"id": t, "name": tname(t), "n": n, "before": b})).collect::<Vec<_>>(),
        "people": people.iter().filter(|r| !r.s("people").is_empty()).map(|r| { let k = r.s("people"); json!({"id": k, "name": pnames.get(&k).map(|x| x.s("name")).unwrap_or(k.clone()), "n": r.i("n")}) }).collect::<Vec<_>>(),
        "partners": partners.iter().filter(|r| r.s("orgs") != id).map(|r| { let k = r.s("orgs"); json!({"id": k, "name": labs.get(&k).map(|x| x.s("name")).unwrap_or(k.clone()), "country": labs.get(&k).map(|x| x.s("country")), "n": r.i("n")}) }).collect::<Vec<_>>(),
        "countries": countries.iter().filter(|r| !r.s("countries").is_empty()).map(|r| json!({"id": r.s("countries"), "n": r.i("n")})).collect::<Vec<_>>(),
        "newest": list(&newest), "cited": list(&cited), "total": total,
    }))
}

/// The organisations of one paper.
pub async fn of_paper(c: &Ctx<'_>, paper: &str) -> Value {
    let r = c.soft("its organisations", "SELECT orgs FROM ax_lab_papers WHERE id = :id", json!({"id": paper})).await;
    let ids: Vec<String> = r.first().map(|r| strs(r, "orgs")).unwrap_or_default();
    let names = named(c, &ids).await;
    json!({"labs": ids.iter().filter_map(|k| names.get(k)).map(|r| json!({"id": r.s("id"), "name": r.s("name"), "country": r.s("country")})).collect::<Vec<_>>()})
}

/// Labs by name.
pub async fn find(c: &Ctx<'_>, q: &str) -> Value {
    let rows = c.soft("labs by name", "SELECT id, name, country, kind, papers, recent FROM ax_labs WHERE MATCH(name, :q, min => :min) ORDER BY papers DESC LIMIT 12", json!({"q": q, "min": crate::search::min_words(q)})).await;
    json!({"labs": rows})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_is_steady_for_small_counts() {
        assert!(growth(100, 50) > growth(10, 5));
        assert!(growth(20, 0) > 5.0);
        assert!(growth(0, 0) < 1.0);
    }
}
