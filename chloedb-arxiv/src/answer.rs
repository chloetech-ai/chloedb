//! Questions a search box asks, answered by SQL with no model: how many
//! papers, who writes on a topic, its trend, two topics side by side, what
//! grows, where a topic began, where it is published, the techniques it
//! uses and the domains a technique serves, and a reading list. Each answer
//! is a sentence with its numbers, the rows behind it and a chart's series,
//! and its statements are in the request's trace.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::concepts;
use crate::search::{self, Ctx};
use crate::strata::{Row, RowExt};
use crate::understand::{Ask, Parsed};

fn fmt(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// What the question is about, as said back: the words, else the filters.
pub fn subject(p: &Parsed) -> String {
    let w = p.words();
    if !w.is_empty() {
        return w;
    }
    // What it is about (a field, a category, a person), not when: the
    // dates are said in the chips ("on robotics", not "on since 2023, in robotics").
    let what = |dates: bool| p.notes.iter().filter(|n| n.kind != "ask" && n.kind != "sort" && (dates || n.kind != "date")).map(|n| n.what.trim_start_matches("in ").to_string()).collect::<Vec<_>>();
    let main = what(false);
    if main.is_empty() { what(true).join(", ") } else { main.join(", ") }
}

/// Papers by year over everything (for shares), from the column.
pub async fn totals(c: &Ctx<'_>) -> HashMap<i64, i64> {
    c.soft("all papers by year (for shares)", "SELECT year, COUNT(*) AS n FROM ax_papers GROUP BY year", Value::Null).await.into_iter().map(|r| (r.i("year"), r.i("n"))).collect()
}

pub fn this_year() -> i64 {
    use chrono::Datelike;
    chrono::Utc::now().year() as i64
}

/// A year's count as a share of all papers that year, in papers per
/// ten thousand.
fn per10k(n: i64, all: i64) -> f64 {
    if all <= 0 {
        0.0
    } else {
        (n as f64 * 10_000.0 / all as f64 * 10.0).round() / 10.0
    }
}

/// Year by year counts of the papers a condition keeps.
pub fn by_year<'a>(c: &Ctx<'a>, what: &str, wh: &str, params: &Map<String, Value>) -> impl std::future::Future<Output = Vec<(i64, i64)>> + 'a {
    let f = c.soft(what, &format!("SELECT year, COUNT(*) AS n FROM ax_papers {wh} GROUP BY year ORDER BY year"), Value::Object(params.clone()));
    async move { f.await.iter().map(|r| (r.i("year"), r.i("n"))).filter(|(y, _)| *y > 1900).collect() }
}

/// The last twelve months against the twelve before, as counts.
pub fn growth<'a>(c: &Ctx<'a>, wh: &str, params: &Map<String, Value>) -> impl std::future::Future<Output = (i64, i64)> + 'a {
    let recent = if wh.is_empty() { "WHERE created >= now() - INTERVAL '365 days'".to_string() } else { format!("{wh} AND created >= now() - INTERVAL '365 days'") };
    let before = if wh.is_empty() { "WHERE created >= now() - INTERVAL '730 days' AND created < now() - INTERVAL '365 days'".to_string() } else { format!("{wh} AND created >= now() - INTERVAL '730 days' AND created < now() - INTERVAL '365 days'") };
    let pr = Value::Object(params.clone());
    let fa = c.soft("in the last twelve months", &format!("SELECT COUNT(*) AS n FROM ax_papers {recent}"), pr.clone());
    let fb = c.soft("in the twelve before", &format!("SELECT COUNT(*) AS n FROM ax_papers {before}"), pr);
    async move {
        let (a, b) = tokio::join!(fa, fb);
        (a.first().map(|r| r.i("n")).unwrap_or(0), b.first().map(|r| r.i("n")).unwrap_or(0))
    }
}

fn series(years: &[(i64, i64)], totals: &HashMap<i64, i64>) -> Value {
    json!(years.iter().map(|(y, n)| json!({"year": y, "n": n, "per10k": per10k(*n, *totals.get(y).unwrap_or(&0))})).collect::<Vec<_>>())
}

fn change(now: i64, before: i64) -> String {
    if before == 0 {
        return if now == 0 { "none in either of the last two years".into() } else { format!("{} in the last twelve months, none in the twelve before", fmt(now)) };
    }
    let r = now as f64 / before as f64;
    let pct = ((r - 1.0) * 100.0).round() as i64;
    if pct.abs() < 3 {
        format!("{} in the last twelve months, about as many as the twelve before ({})", fmt(now), fmt(before))
    } else if pct > 0 {
        format!("{} in the last twelve months, up {pct}% from {}", fmt(now), fmt(before))
    } else {
        format!("{} in the last twelve months, down {}% from {}", fmt(now), -pct, fmt(before))
    }
}

/// The share of all papers the topic had in its best year, and when.
fn peak(years: &[(i64, i64)], totals: &HashMap<i64, i64>) -> Option<(i64, f64)> {
    let now = this_year();
    years.iter().filter(|(y, _)| *y < now).map(|(y, n)| (*y, per10k(*n, *totals.get(y).unwrap_or(&0)))).max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

fn list(rows: &[Row]) -> Value {
    json!(rows)
}

/// Answers the question a search asks, if it asks one. `papers` are the
/// search's own results (ordered as the question wants, for Top, Survey,
/// Origin).
pub async fn answer(c: &Ctx<'_>, p: &Parsed, papers: &[Row], total: i64) -> Option<Value> {
    let ask = p.ask.clone()?;
    let subj = subject(p);
    let mut params = Map::new();
    let wh = search::about(p, &mut params);
    match ask {
        Ask::Top { by, one } => {
            let first = papers.first()?;
            let text = match (by.as_str(), one) {
                ("cites", _) => format!("The most cited paper on {subj} is “{}” ({}, {} citations){}.", first.s("title"), first.i("year"), fmt(first.i("cites")), if one { String::new() } else { format!(", of {} papers", fmt(total)) }),
                ("date", _) => format!("The newest paper on {subj} is “{}”, posted {}.", first.s("title"), first.s("created").get(..10).unwrap_or("")),
                _ => format!("The earliest paper on {subj} here is “{}” ({}).", first.s("title"), first.i("year")),
            };
            Some(json!({"kind": "top", "text": text, "by": by}))
        }
        Ask::Count { per } => {
            let tot = totals(c).await;
            let (years, (now, before)) = tokio::join!(by_year(c, "papers on it by year", &wh, &params), growth(c, &wh, &params));
            let best = years.iter().max_by_key(|(_, n)| *n).copied();
            let mut text = format!("{} papers on {subj}", fmt(total));
            if let Some((y, n)) = best {
                text.push_str(&format!(", the most in {y} ({})", fmt(n)));
            }
            text.push_str(&format!("; {}.", change(now, before)));
            let months = if per == "month" {
                let rows = c.soft("papers on it by month, the last three years", &format!("SELECT month, COUNT(*) AS n FROM ax_papers {} GROUP BY month ORDER BY month", if wh.is_empty() { "WHERE created >= now() - INTERVAL '1095 days'".to_string() } else { format!("{wh} AND created >= now() - INTERVAL '1095 days'") }), Value::Object(params.clone())).await;
                json!(rows.iter().map(|r| json!({"month": r.s("month"), "n": r.i("n")})).collect::<Vec<_>>())
            } else {
                Value::Null
            };
            Some(json!({"kind": "count", "text": text, "total": total, "years": series(&years, &tot), "months": months, "recent": now, "before": before}))
        }
        Ask::People { by } => {
            let n = p.limit.unwrap_or(15).min(50);
            let rows = if by == "cites" {
                c.soft("who is cited most on it", &format!("SELECT people, SUM(cites) AS s, COUNT(*) AS n FROM ax_papers {wh} GROUP BY people ORDER BY s DESC LIMIT {n}"), Value::Object(params.clone())).await
            } else {
                people_by_count(c, "who writes most on it", &wh, &params, n).await
            };
            let keys: Vec<String> = rows.iter().map(|r| r.s("people")).filter(|k| !k.is_empty()).collect();
            let names = search::names(c, &keys).await;
            let people: Vec<Value> = rows.iter().filter(|r| !r.s("people").is_empty()).map(|r| {
                let k = r.s("people");
                let a = names.get(&k);
                json!({"id": k, "name": a.map(|a| a.s("name")).unwrap_or_else(|| k.clone()), "n": r.i("n"), "cites": r.i("s"), "papers": a.map(|a| a.i("papers")).unwrap_or(0), "h": a.map(|a| a.i("h")).unwrap_or(0)})
            }).collect();
            let top = people.first()?;
            let text = if by == "cites" {
                format!("{} is the most cited author on {subj}: {} citations over {} papers on it.", top["name"].as_str().unwrap_or(""), fmt(top["cites"].as_i64().unwrap_or(0)), top["n"])
            } else {
                format!("{} has written the most papers on {subj}: {} of {}{}.", top["name"].as_str().unwrap_or(""), top["n"], fmt(total), people.get(1).map(|x| format!(", then {} ({})", x["name"].as_str().unwrap_or(""), x["n"])).unwrap_or_default())
            };
            Some(json!({"kind": "people", "text": text, "people": people, "by": by}))
        }
        Ask::Coauthors => {
            let who = p.authors.first()?.clone();
            let rows = c.gsoft("who they wrote with (the coauthor graph)", "SELECT src, dst, w FROM ax_coauthors WHERE src = :a OR dst = :a ORDER BY w DESC LIMIT 30", json!({"a": who})).await;
            let others: Vec<(String, i64)> = rows.iter().map(|r| (if r.s("src") == who { r.s("dst") } else { r.s("src") }, r.f("w") as i64)).collect();
            let keys: Vec<String> = others.iter().map(|o| o.0.clone()).chain([who.clone()]).collect();
            let names = search::names(c, &keys).await;
            let me = names.get(&who).map(|r| r.s("name")).unwrap_or(who.clone());
            let people: Vec<Value> = others.iter().map(|(k, w)| json!({"id": k, "name": names.get(k).map(|r| r.s("name")).unwrap_or(k.clone()), "n": w, "papers": names.get(k).map(|r| r.i("papers")).unwrap_or(0)})).collect();
            let text = match people.first() {
                Some(t) => format!("{me} has written most often with {} ({} papers together){}.", t["name"].as_str().unwrap_or(""), t["n"], if people.len() > 1 { format!(", among {} people they wrote two papers or more with", people.len()) } else { String::new() }),
                None => format!("{me} has not written two papers with the same person here."),
            };
            Some(json!({"kind": "coauthors", "text": text, "person": {"id": who, "name": me}, "people": people}))
        }
        Ask::Trend => {
            let tot = totals(c).await;
            let (years, (now, before)) = tokio::join!(by_year(c, "papers on it by year", &wh, &params), growth(c, &wh, &params));
            let mut text = format!("{subj}: {}.", change(now, before));
            if let Some((y, s)) = peak(&years, &tot) {
                text.push_str(&format!(" Its largest share of arXiv was in {y}, {s} papers in every 10,000."));
            }
            let first = years.first().map(|y| y.0);
            Some(json!({"kind": "trend", "text": text, "years": series(&years, &tot), "recent": now, "before": before, "first": first}))
        }
        Ask::Compare { terms } => {
            let tot = totals(c).await;
            let mut futs = Vec::new();
            for t in &terms {
                let mut q = p.clone();
                q.text = t.clone();
                q.phrases.clear();
                q.ask = None;
                let mut pr = Map::new();
                let w = search::about(&q, &mut pr);
                let fy = by_year(c, &format!("“{t}” by year"), &w, &pr);
                let fg = growth(c, &w, &pr);
                let t = t.clone();
                futs.push(async move {
                    let (ys, g) = tokio::join!(fy, fg);
                    (t, ys, g)
                });
            }
            let got = futures_util::future::join_all(futs).await;
            let mut text = Vec::new();
            let mut out = Vec::new();
            for (t, ys, (now, before)) in &got {
                let total: i64 = ys.iter().map(|y| y.1).sum();
                text.push(format!("{t}: {} papers, {}", fmt(total), change(*now, *before)));
                out.push(json!({"term": t, "total": total, "recent": now, "before": before, "years": series(ys, &tot)}));
            }
            let lead = got.iter().max_by_key(|g| g.2 .0).map(|g| g.0.clone()).unwrap_or_default();
            Some(json!({"kind": "compare", "text": format!("{}. {lead} has the most papers in the last twelve months.", text.join("; ")), "terms": out}))
        }
        Ask::Hot => {
            // Within what the query is about (its filters and its words):
            // what grew most from the twelve months before to the last twelve.
            let scoped = !wh.is_empty();
            let join = |extra: &str| if wh.is_empty() { format!("WHERE {extra}") } else { format!("{wh} AND {extra}") };
            let recent = join("created >= now() - INTERVAL '365 days'");
            let before = join("created >= now() - INTERVAL '730 days' AND created < now() - INTERVAL '365 days'");
            let pr = Value::Object(params.clone());
            let (tr, tb, kr, kb, or, ob) = tokio::join!(
                c.soft("techniques, the last twelve months", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers {recent} GROUP BY techniques ORDER BY n DESC LIMIT 200"), pr.clone()),
                c.soft("techniques, the twelve before", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers {before} GROUP BY techniques ORDER BY n DESC LIMIT 200"), pr.clone()),
                c.soft("domains, the last twelve months", &format!("SELECT domains, COUNT(*) AS n FROM ax_papers {recent} GROUP BY domains ORDER BY n DESC LIMIT 200"), pr.clone()),
                c.soft("domains, the twelve before", &format!("SELECT domains, COUNT(*) AS n FROM ax_papers {before} GROUP BY domains ORDER BY n DESC LIMIT 200"), pr.clone()),
                async { if scoped { c.soft("topics, the last twelve months", &format!("SELECT topic, COUNT(*) AS n FROM ax_papers {recent} GROUP BY topic ORDER BY n DESC LIMIT 300"), pr.clone()).await } else { Vec::new() } },
                async { if scoped { c.soft("topics, the twelve before", &format!("SELECT topic, COUNT(*) AS n FROM ax_papers {before} GROUP BY topic ORDER BY n DESC LIMIT 300"), pr.clone()).await } else { Vec::new() } },
            );
            // A subject has fewer papers than arXiv: fewer make a trend.
            let least = if scoped { 5 } else { 20 };
            let rise = |a: &[Row], b: &[Row], col: &str| -> Vec<(String, i64, i64, f64)> {
                let before: HashMap<String, i64> = b.iter().map(|r| (r.s(col), r.i("n"))).collect();
                let all_a: i64 = a.iter().map(|r| r.i("n")).sum::<i64>().max(1);
                let all_b: i64 = b.iter().map(|r| r.i("n")).sum::<i64>().max(1);
                let mut v: Vec<(String, i64, i64, f64)> = a.iter().filter(|r| !r.s(col).is_empty() && r.i("n") >= least).filter_map(|r| {
                    let k = r.s(col);
                    let n = r.i("n");
                    let m = *before.get(&k).unwrap_or(&0);
                    // Share against share, smoothed so a handful is not a boom.
                    let g = ((n as f64 + 5.0) / all_a as f64) / ((m as f64 + 5.0) / all_b as f64);
                    // Only a rise the counts bear out: the growth's lower end
                    // (counts as Poisson, 90% one-sided) above 1, so 16 papers
                    // after 2 is not ranked over 369 after 145.
                    let low = g.ln() - 1.645 * (1.0 / (n as f64 + 0.5) + 1.0 / (m as f64 + 0.5)).sqrt();
                    (low > 0.0).then(|| (k, n, m, (g * 100.0).round() / 100.0))
                }).collect();
                // Fastest first, as the answer says.
                v.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal).then(b.1.cmp(&a.1)));
                v.truncate(12);
                v
            };
            let named = |v: Vec<(String, i64, i64, f64)>| -> Vec<Value> {
                v.into_iter().map(|(k, n, m, g)| {
                    let name = concepts::dict().by_slug(&k).map(|c| c.name.clone()).unwrap_or(k.clone());
                    json!({"id": k, "name": name, "recent": n, "before": m, "growth": g})
                }).collect()
            };
            let techs = named(rise(&tr, &tb, "techniques"));
            let doms = named(rise(&kr, &kb, "domains"));
            let topics = if scoped {
                // The subject's own topics, by the same measure.
                let up = rise(&or, &ob, "topic");
                let ids: Vec<i64> = up.iter().filter_map(|x| x.0.parse().ok()).collect();
                let rows = if ids.is_empty() { Vec::new() } else { c.soft("those topics' names", "SELECT id, label, about, papers, field_label FROM ax_topics WHERE id IN (:ids)", json!({"ids": ids})).await };
                let by: HashMap<String, &Row> = rows.iter().map(|r| (r.s("id"), r)).collect();
                up.into_iter().filter_map(|(k, n, m, g)| by.get(&k).map(|r| json!({"id": k, "label": r.s("label"), "about": r.s("about"), "papers": r.i("papers"), "field_label": r.s("field_label"), "recent": n, "before": m, "growth": g}))).collect::<Vec<_>>()
            } else {
                c.soft("the topics growing fastest", "SELECT id, label, about, papers, recent, growth, field_label FROM ax_topics WHERE recent >= 100 ORDER BY growth DESC LIMIT 12", Value::Null).await.into_iter().map(Value::Object).collect()
            };
            let scope = if subj.is_empty() { "arXiv".to_string() } else { subj.trim_start_matches("in ").to_string() };
            let names: Vec<String> = techs.iter().take(3).map(|t| format!("{} ({}×)", t["name"].as_str().unwrap_or(""), t["growth"])).collect();
            let text = if names.is_empty() {
                format!("No technique grew clearly in {scope} over the last twelve months: too few papers, or none rose beyond what chance would give.")
            } else {
                format!("Growing fastest in {scope} over the last twelve months: {}.", names.join(", "))
            };
            Some(json!({"kind": "hot", "text": text, "techniques": techs, "domains": doms, "topics": topics}))
        }
        Ask::Origin => {
            let pr = Value::Object(params.clone());
            let cited = if wh.is_empty() { "WHERE cites >= 100".to_string() } else { format!("{wh} AND cites >= 100") };
            let (first, famous) = tokio::join!(
                c.soft("the earliest papers on it", &format!("SELECT {} FROM ax_papers {wh} ORDER BY created ASC LIMIT 6", search::COLS), pr.clone()),
                c.soft("the earliest cited 100 times or more", &format!("SELECT {} FROM ax_papers {cited} ORDER BY created ASC LIMIT 6", search::COLS), pr.clone()),
            );
            let f = first.first()?;
            let mut text = format!("The earliest paper on {subj} on arXiv is “{}” ({}, {}).", f.s("title"), f.i("year"), first_author(f));
            if let Some(x) = famous.first() {
                text.push_str(&format!(" The first to be cited 100 times or more is “{}” ({}, {}, {} citations).", x.s("title"), x.i("year"), first_author(x), fmt(x.i("cites"))));
            }
            Some(json!({"kind": "origin", "text": text, "first": list(&first), "famous": list(&famous)}))
        }
        Ask::Survey => {
            let n = papers.len();
            let text = if n == 0 { format!("No surveys of {subj} were found.") } else { format!("{} surveys and reviews of {subj}; the most cited is “{}” ({}).", fmt(total), papers[0].s("title"), papers[0].i("year")) };
            Some(json!({"kind": "survey", "text": text}))
        }
        Ask::Summary => Some(json!({"kind": "summary", "text": format!("A summary of {subj}, from its papers, is written below."), "pending": true, "subject": subj})),
        Ask::Venues => {
            let rows = c.soft("where it is published", &format!("SELECT venue, COUNT(*) AS n, SUM(cites) AS s FROM ax_papers {wh} GROUP BY venue ORDER BY n DESC LIMIT 16"), Value::Object(params.clone())).await;
            let rows: Vec<Value> = rows.iter().filter(|r| !r.s("venue").is_empty()).map(|r| json!({"venue": r.s("venue"), "n": r.i("n"), "cites": r.i("s")})).collect();
            let top = rows.first()?;
            let named: i64 = rows.iter().map(|r| r["n"].as_i64().unwrap_or(0)).sum();
            Some(json!({"kind": "venues", "text": format!("Papers on {subj} that name where they were published appear most at {} ({} papers){}; most papers on arXiv name no venue.", top["venue"].as_str().unwrap_or(""), top["n"], rows.get(1).map(|x| format!(", then {} ({})", x["venue"].as_str().unwrap_or(""), x["n"])).unwrap_or_default()), "venues": rows, "named": named}))
        }
        Ask::Techniques | Ask::Applications => {
            let col = if matches!(ask, Ask::Techniques) { "techniques" } else { "domains" };
            let rows = c.soft(if col == "techniques" { "the techniques its papers use" } else { "the domains its papers serve" }, &format!("SELECT {col}, COUNT(*) AS n FROM ax_papers {wh} GROUP BY {col} ORDER BY n DESC LIMIT 20"), Value::Object(params.clone())).await;
            let own: Vec<String> = p.concepts.clone();
            let items: Vec<Value> = rows.iter().filter(|r| !r.s(col).is_empty() && !own.contains(&r.s(col))).take(14).map(|r| {
                let k = r.s(col);
                json!({"id": k, "name": concepts::dict().by_slug(&k).map(|c| c.name.clone()).unwrap_or(k.clone()), "n": r.i("n"), "share": if total > 0 { (r.i("n") as f64 * 1000.0 / total as f64).round() / 10.0 } else { 0.0 }})
            }).collect();
            let names: Vec<String> = items.iter().take(4).map(|x| format!("{} ({}%)", x["name"].as_str().unwrap_or(""), x["share"])).collect();
            let text = if col == "techniques" { format!("Papers on {subj} most often use {}.", names.join(", ")) } else { format!("{subj} is applied most in {}.", names.join(", ")) };
            Some(json!({"kind": col, "text": text, "items": items}))
        }
        Ask::ReadingList => {
            let pr = Value::Object(params.clone());
            let join = |extra: &str| if wh.is_empty() { format!("WHERE {extra}") } else { format!("{wh} AND {extra}") };
            let (surveys, classics, recent, newest) = tokio::join!(
                c.soft("start with a survey", &format!("SELECT {} FROM ax_papers {} ORDER BY cites DESC LIMIT 3", search::COLS, join("kinds = 'survey'")), pr.clone()),
                c.soft("the foundations: most cited", &format!("SELECT {} FROM ax_papers {wh} ORDER BY cites DESC LIMIT 6", search::COLS), pr.clone()),
                c.soft("the last two years' most cited", &format!("SELECT {} FROM ax_papers {} ORDER BY cites DESC LIMIT 4", search::COLS, join("created >= now() - INTERVAL '730 days'")), pr.clone()),
                c.soft("and the newest", &format!("SELECT {} FROM ax_papers {} ORDER BY created DESC LIMIT 3", search::COLS, join("created >= now() - INTERVAL '60 days'")), pr.clone()),
            );
            let mut seen = std::collections::HashSet::new();
            let mut steps = Vec::new();
            for (name, rows) in [("Start with a survey", &surveys), ("The foundations", &classics), ("What moved it lately", &recent), ("The newest", &newest)] {
                let mut keep: Vec<Row> = rows.iter().filter(|r| seen.insert(r.s("id"))).cloned().collect();
                // The foundations in the order they were written.
                if name == "The foundations" {
                    keep.sort_by_key(|r| r.s("created"));
                }
                if !keep.is_empty() {
                    steps.push(json!({"step": name, "papers": keep}));
                }
            }
            let n: usize = steps.iter().map(|s| s["papers"].as_array().map(|a| a.len()).unwrap_or(0)).sum();
            Some(json!({"kind": "reading_list", "text": format!("A reading list for {subj}: {n} papers, from a survey and the foundations to the newest."), "steps": steps}))
        }
    }
}

pub fn first_author(r: &Row) -> String {
    let a = r.s("authors");
    let first = a.split(", ").next().unwrap_or("").to_string();
    let n = r.i("nauthors");
    if n > 2 {
        format!("{first} et al.")
    } else if n == 2 {
        a.replace(", ", " and ")
    } else {
        first
    }
}


/// Who writes most where `wh` holds (`WHERE ...`, or nothing), with the
/// citations of their papers there (`s`). Grouping by people and summing
/// citations in one statement takes about 0.7 s over a topic's papers, each
/// with many people; counting alone is answered from the postings in under
/// 0.1 s, and one person's sum in a few milliseconds, so the sums follow,
/// side by side, for the few counted.
pub async fn people_by_count(c: &Ctx<'_>, what: &str, wh: &str, params: &Map<String, Value>, n: usize) -> Vec<Row> {
    let mut rows = c.soft(what, &format!("SELECT people, COUNT(*) AS n FROM ax_papers {wh} GROUP BY people ORDER BY n DESC LIMIT {n}"), Value::Object(params.clone())).await;
    let and = if wh.trim().is_empty() { "WHERE" } else { "AND" };
    let sql = format!("SELECT SUM(cites) AS s FROM ax_papers {wh} {and} people = :who_");
    let sums = futures_util::future::join_all(rows.iter().map(|r| {
        let mut p = params.clone();
        p.insert("who_".into(), json!(r.s("people")));
        c.soft("their citations there, one person at a time", &sql, Value::Object(p))
    }))
    .await;
    for (r, s) in rows.iter_mut().zip(sums) {
        r.insert("s".into(), json!(s.first().map(|x| x.i("s")).unwrap_or(0)));
    }
    rows
}
