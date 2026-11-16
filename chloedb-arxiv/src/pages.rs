//! The pages beyond a search: a paper (what it cites, who cites it, what to
//! read next), a person (papers over time, coauthors, topics), a topic, a
//! technique or domain, a category, a venue, the atlas of topics, and a
//! reader's own recommendations. Each is a handful of statements run at
//! once.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::answer::{self, by_year, totals};
use crate::concepts;
use crate::schema::RECOMMENDER;
use crate::search::{self, Ctx, COLS, COLS_ABS};
use crate::strata::{Row, RowExt};

fn parse_by_year(s: &str) -> Value {
    serde_json::from_str::<Value>(s).unwrap_or(Value::Null)
}

fn ids_of(rows: &[Row], col: &str) -> Vec<String> {
    rows.iter().map(|r| r.s(col)).filter(|s| !s.is_empty()).collect()
}

/// A walk of a graph on the graph server: the nodes PageRank from the
/// seeds reaches, best first, the seeds left out.
pub fn walk<'a>(c: &Ctx<'a>, what: &str, edges: &str, seeds: &[String], k: usize) -> impl std::future::Future<Output = Vec<String>> + 'a {
    let seeds = seeds.to_vec();
    let f = (!seeds.is_empty()).then(|| c.gsoft(what, &format!("SELECT id, score() AS score FROM ax_nodes WHERE id NOT IN (:seeds) ORDER BY ppr({edges}, :seeds) DESC LIMIT {k}"), json!({"seeds": seeds})));
    async move {
        let Some(f) = f else { return Vec::new() };
        f.await.into_iter().filter(|r| r.f("score") > 0.0).map(|r| r.s("id")).collect()
    }
}

/// Papers ranked by fusing lists of keys (`ranked`) with the nearest by
/// meaning to a paper, on the search server.
fn fused<'a>(c: &Ctx<'a>, what: &str, lists: &[Vec<String>], near: Option<&str>, skip: &[String], filter: Option<&str>, params: serde_json::Map<String, Value>, k: usize) -> impl std::future::Future<Output = Vec<Row>> + 'a {
    let mut p = params;
    let mut parts = Vec::new();
    for (i, l) in lists.iter().filter(|l| !l.is_empty()).enumerate() {
        p.insert(format!("l{i}"), json!(l.iter().take(300).collect::<Vec<_>>()));
        parts.push(format!("ranked(:l{i})"));
    }
    if let Some(n) = near {
        p.insert("v".into(), json!({"vector_of": n, "table": "ax_papers", "field": "emb"}));
        parts.push("similarity(emb, :v)".to_string());
    }
    let mut wh: Vec<String> = Vec::new();
    if !skip.is_empty() {
        p.insert("skip".into(), json!(skip));
        wh.push("id NOT IN (:skip)".into());
    }
    if let Some(f) = filter {
        wh.push(f.to_string());
    }
    let wh = if wh.is_empty() { String::new() } else { format!("WHERE {}", wh.join(" AND ")) };
    let f = (!parts.is_empty()).then(|| {
        let order = if parts.len() == 1 { parts[0].clone() } else { format!("rrf({})", parts.join(", ")) };
        c.soft(what, &format!("SELECT {COLS}, score() AS score FROM ax_papers {wh} ORDER BY {order} DESC LIMIT {k}"), Value::Object(p))
    });
    async move {
        match f {
            Some(f) => f.await,
            None => Vec::new(),
        }
    }
}

/// Papers by key, most cited first.
fn papers_by_cites<'a>(c: &Ctx<'a>, what: &str, ids: &[String], k: usize) -> impl std::future::Future<Output = Vec<Row>> + 'a {
    let ids: Vec<String> = ids.iter().take(10_000).cloned().collect();
    let f = (!ids.is_empty()).then(|| c.soft(what, &format!("SELECT {COLS} FROM ax_papers WHERE id IN (:ids) ORDER BY cites DESC LIMIT {k}"), json!({"ids": ids})));
    async move {
        match f {
            Some(f) => f.await,
            None => Vec::new(),
        }
    }
}

/// Papers by key, in the order given.
pub fn papers_by_id<'a>(c: &Ctx<'a>, what: &str, ids: &[String], cols: &str) -> impl std::future::Future<Output = Vec<Row>> + 'a {
    let ids = ids.to_vec();
    let f = (!ids.is_empty()).then(|| c.soft(what, &format!("SELECT {cols} FROM ax_papers WHERE id IN (:ids)"), json!({"ids": ids})));
    async move {
        let Some(f) = f else { return Vec::new() };
        let mut by: HashMap<String, Row> = f.await.into_iter().map(|r| (r.s("id"), r)).collect();
        ids.iter().filter_map(|i| by.remove(i)).collect()
    }
}

/// A paper: its row, what it cites and who cites it on arXiv, the papers
/// cited with it (the recommender's counts: what readers who cite it also
/// cite), the nearest by meaning, and "read next", the three fused.
pub async fn paper(c: &Ctx<'_>, id: &str) -> Option<Value> {
    let id = crate::understand::clean_id(id);
    let p = json!({"id": id});
    let v = json!({"id": id, "v": {"vector_of": id, "table": "ax_papers", "field": "emb"}});
    let counts = format!("{RECOMMENDER}_counts");
    let me = vec![id.clone()];
    let (row, refs, cited, co, similar, next, walked) = tokio::join!(
        c.soft("the paper", &format!("SELECT {COLS_ABS}, comments, license, journal FROM ax_papers WHERE id = :id"), p.clone()),
        c.gsoft("what it cites (on arXiv)", "SELECT dst FROM ax_cites WHERE src = :id LIMIT 10000", p.clone()),
        c.gsoft("who cites it (on arXiv)", "SELECT src FROM ax_cites WHERE dst = :id LIMIT 10000", p.clone()),
        c.gsoft("cited together with it (the recommender's counts)", &format!("SELECT dst, co, next FROM {counts} WHERE kind = 'pair' AND src = :id ORDER BY co DESC LIMIT 40"), p.clone()),
        c.soft("the nearest by meaning", &format!("SELECT {COLS}, score() AS score FROM ax_papers WHERE id != :id ORDER BY similarity(emb, :v) DESC LIMIT 12"), v.clone()),
        c.gsoft("what came after it in readers' lists (the recommender's counts)", &format!("SELECT dst, next FROM {counts} WHERE kind = 'pair' AND src = :id AND next > 0 ORDER BY next DESC LIMIT 20"), p.clone()),
        walk(c, "PageRank from it over the citations", "ax_cites", &me, 60),
    );
    let mut row = row.into_iter().next()?;
    let co_ids = ids_of(&co, "dst");
    let next_ids = ids_of(&next, "dst");
    let (ref_ids, cited_ids) = (ids_of(&refs, "dst"), ids_of(&cited, "src"));
    // Read next: what readers cite with it, the nearest by meaning and a
    // walk of the citation graph from it, fused by rank.
    let co_list: Vec<String> = co_ids.iter().take(30).cloned().collect();
    let co_top: Vec<String> = co_ids.iter().take(12).cloned().collect();
    let next_top: Vec<String> = next_ids.iter().take(8).cloned().collect();
    let (read_next, refs, cited, co_rows, next_rows, topic, people) = tokio::join!(
        fused(c, "read next: cited with it, nearest by meaning and PageRank from it over citations, fused (rrf)", &[co_list, walked], Some(&id), &me, None, serde_json::Map::new(), 12),
        papers_by_cites(c, "what it cites, most cited first", &ref_ids, 40),
        papers_by_cites(c, "who cites it, most cited first", &cited_ids, 40),
        papers_by_id(c, "the papers cited with it", &co_top, COLS),
        papers_by_id(c, "the papers after it", &next_top, COLS),
        async {
            let t = row.i("topic");
            if row.get("topic").is_some_and(|x| !x.is_null()) {
                c.soft("its topic", "SELECT id, label, about, papers, recent, growth, field_label FROM ax_topics WHERE id = :t", json!({"t": t})).await
            } else {
                Vec::new()
            }
        },
        async {
            let keys: Vec<String> = row.get("people").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).take(30).collect()).unwrap_or_default();
            search::names(c, &keys).await
        },
    );
    let authors: Vec<Value> = row.get("people").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).take(30).map(|k| {
        let r = people.get(k);
        json!({"id": k, "name": r.map(|r| r.s("name")).unwrap_or(k.to_string()), "papers": r.map(|r| r.i("papers")).unwrap_or(0), "h": r.map(|r| r.i("h")).unwrap_or(0)})
    }).collect()).unwrap_or_default();
    row.insert("authors_list".into(), json!(authors));
    let co_n: HashMap<String, i64> = co.iter().map(|r| (r.s("dst"), r.f("co") as i64)).collect();
    let mut co_rows = co_rows;
    for r in co_rows.iter_mut() {
        let k = r.s("id");
        r.insert("together".into(), json!(co_n.get(&k).copied().unwrap_or(0)));
    }
    let dict = concepts::dict();
    let named = |col: &str| -> Value {
        json!(row.get(col).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| json!({"id": s, "name": dict.by_slug(s).map(|c| c.name.clone()).unwrap_or(s.to_string())})).collect::<Vec<_>>()).unwrap_or_default())
    };
    let cats: Vec<Value> = row.get("cats").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| json!({"id": s, "name": concepts::category_name(s)})).collect()).unwrap_or_default();
    Some(json!({
        "paper": row,
        "techniques": named("techniques"), "domains": named("domains"), "categories": cats,
        "topic": topic.first(),
        "references": refs, "cited_by": cited,
        "read_next": read_next, "cited_with": co_rows, "after": next_rows, "similar": similar,
        "links": {"abs": format!("https://arxiv.org/abs/{id}"), "pdf": format!("https://arxiv.org/pdf/{id}"), "html": format!("https://arxiv.org/html/{id}")},
    }))
}

/// A person: their row, papers (most cited and newest), papers a year,
/// the people they write with most, their topics and techniques, and the
/// people nearest them in the coauthor graph.
pub async fn author(c: &Ctx<'_>, id: &str) -> Option<Value> {
    let p = json!({"a": id});
    let (row, top, newest, years, co, topics, techs, cats, first, by_year) = tokio::join!(
        c.soft("the person", "SELECT * FROM ax_authors WHERE id = :a", p.clone()),
        c.soft("their most cited papers", &format!("SELECT {COLS} FROM ax_papers WHERE people = :a ORDER BY cites DESC LIMIT 20"), p.clone()),
        c.soft("their newest papers", &format!("SELECT {COLS} FROM ax_papers WHERE people = :a ORDER BY created DESC LIMIT 12"), p.clone()),
        c.soft("their papers a year", "SELECT year, COUNT(*) AS n, SUM(cites) AS s FROM ax_papers WHERE people = :a GROUP BY year ORDER BY year", p.clone()),
        c.gsoft("who they write with (the coauthor graph)", "SELECT src, dst, w FROM ax_coauthors WHERE src = :a OR dst = :a ORDER BY w DESC LIMIT 24", p.clone()),
        c.soft("their topics", "SELECT topic, COUNT(*) AS n FROM ax_papers WHERE people = :a GROUP BY topic ORDER BY n DESC LIMIT 8", p.clone()),
        c.soft("their techniques", "SELECT techniques, COUNT(*) AS n FROM ax_papers WHERE people = :a GROUP BY techniques ORDER BY n DESC LIMIT 10", p.clone()),
        c.soft("their categories", "SELECT cats, COUNT(*) AS n FROM ax_papers WHERE people = :a GROUP BY cats ORDER BY n DESC LIMIT 8", p.clone()),
        c.soft("their papers as first author", "SELECT COUNT(*) AS n FROM ax_papers WHERE first_author = :a", p.clone()),
        c.soft("their techniques year by year", "SELECT year, techniques, COUNT(*) AS n FROM ax_papers WHERE people = :a GROUP BY year, techniques LIMIT 10000", p.clone()),
    );
    let row = row.into_iter().next()?;
    let others: Vec<(String, i64)> = co.iter().map(|r| (if r.s("src") == id { r.s("dst") } else { r.s("src") }, r.f("w") as i64)).collect();
    let keys: Vec<String> = others.iter().map(|o| o.0.clone()).collect();
    let topic_ids: Vec<i64> = topics.iter().filter(|r| r.get("topic").is_some_and(|v| !v.is_null())).map(|r| r.i("topic")).collect();
    let me = vec![id.to_string()];
    let (names, topic_rows, orbit_ids) = tokio::join!(
        search::names(c, &keys),
        async { if topic_ids.is_empty() { Vec::new() } else { c.soft("the topics' names", "SELECT id, label, field_label, papers FROM ax_topics WHERE id IN (:t)", json!({"t": topic_ids})).await } },
        walk(c, "the people nearest them (PageRank over the coauthor graph)", "ax_coauthors", &me, 12),
    );
    let orbit = if orbit_ids.is_empty() {
        Vec::new()
    } else {
        let rows = c.soft("their rows", "SELECT id, name, papers, cites, h FROM ax_authors WHERE id IN (:ids)", json!({"ids": orbit_ids})).await;
        let mut by: HashMap<String, Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
        orbit_ids.iter().filter_map(|i| by.remove(i)).collect::<Vec<_>>()
    };
    let tname: HashMap<i64, Row> = topic_rows.into_iter().map(|r| (r.i("id"), r)).collect();
    let coauthors: Vec<Value> = others.iter().map(|(k, w)| json!({"id": k, "name": names.get(k).map(|r| r.s("name")).unwrap_or(k.clone()), "n": w, "papers": names.get(k).map(|r| r.i("papers")).unwrap_or(0)})).collect();
    let dict = concepts::dict();
    // Where their work went: their papers a year using each of their seven
    // most used techniques (a paper counts once in each it uses).
    let river_ids: Vec<String> = techs.iter().map(|r| r.s("techniques")).filter(|k| !k.is_empty()).take(7).collect();
    let mut ys: Vec<i64> = by_year.iter().map(|r| r.i("year")).filter(|y| *y > 1990).collect();
    ys.sort();
    ys.dedup();
    let river_years: Vec<Value> = ys
        .iter()
        .map(|y| {
            let mut n = vec![0i64; river_ids.len()];
            for r in by_year.iter().filter(|r| r.i("year") == *y) {
                if let Some(at) = river_ids.iter().position(|t| *t == r.s("techniques")) {
                    n[at] += r.i("n");
                }
            }
            json!({"year": y, "n": n})
        })
        .collect();
    let river = json!({
        "topics": river_ids.iter().map(|t| json!({"id": t, "label": dict.by_slug(t).map(|c| c.name.clone()).unwrap_or(t.clone())})).collect::<Vec<_>>(),
        "years": river_years,
    });
    Some(json!({
        "author": row,
        "river": river,
        "top": top, "newest": newest,
        "years": years.iter().map(|r| json!({"year": r.i("year"), "n": r.i("n"), "cites": r.i("s")})).collect::<Vec<_>>(),
        "coauthors": coauthors,
        "orbit": orbit,
        "topics": topics.iter().filter_map(|r| { let t = r.i("topic"); tname.get(&t).map(|x| json!({"id": t, "label": x.s("label"), "field": x.s("field_label"), "n": r.i("n")})) }).collect::<Vec<_>>(),
        "techniques": techs.iter().filter(|r| !r.s("techniques").is_empty()).map(|r| { let k = r.s("techniques"); json!({"id": k, "name": dict.by_slug(&k).map(|c| c.name.clone()).unwrap_or(k.clone()), "n": r.i("n")}) }).collect::<Vec<_>>(),
        "categories": cats.iter().filter(|r| !r.s("cats").is_empty()).map(|r| { let k = r.s("cats"); json!({"id": k, "name": concepts::category_name(&k), "n": r.i("n")}) }).collect::<Vec<_>>(),
        "first_author": first.first().map(|r| r.i("n")).unwrap_or(0),
    }))
}

/// The common part of a topic, a concept and a category: counts by year
/// (with shares), growth, the most cited and newest papers, the most
/// cited this year, the people, and what else its papers are about.
async fn scope(c: &Ctx<'_>, cond: &str, params: Value, skip_concept: Option<&str>) -> Value {
    let pmap = params.as_object().cloned().unwrap_or_default();
    let wh = format!("WHERE {cond}");
    let recent = format!("{wh} AND created >= now() - INTERVAL '365 days'");
    let (tot, years, (now, before), top, newest, rising, people, techs, doms, cats) = tokio::join!(
        totals(c),
        by_year(c, "papers a year", &wh, &pmap),
        answer::growth(c, &wh, &pmap),
        c.soft("the most cited papers", &format!("SELECT {COLS} FROM ax_papers {wh} ORDER BY cites DESC LIMIT 15"), params.clone()),
        c.soft("the newest papers", &format!("SELECT {COLS} FROM ax_papers {wh} ORDER BY created DESC LIMIT 15"), params.clone()),
        c.soft("the most cited of the last twelve months", &format!("SELECT {COLS} FROM ax_papers {recent} ORDER BY cites DESC LIMIT 10"), params.clone()),
        answer::people_by_count(c, "who writes most on it", &wh, &pmap, 15),
        c.soft("its techniques", &format!("SELECT techniques, COUNT(*) AS n FROM ax_papers {wh} GROUP BY techniques ORDER BY n DESC LIMIT 16"), params.clone()),
        c.soft("its domains", &format!("SELECT domains, COUNT(*) AS n FROM ax_papers {wh} GROUP BY domains ORDER BY n DESC LIMIT 16"), params.clone()),
        c.soft("its categories", &format!("SELECT cats, COUNT(*) AS n FROM ax_papers {wh} GROUP BY cats ORDER BY n DESC LIMIT 10"), params.clone()),
    );
    let keys = ids_of(&people, "people");
    let names = search::names(c, &keys).await;
    let dict = concepts::dict();
    let named = |rows: &[Row], col: &str| -> Vec<Value> {
        rows.iter().filter(|r| !r.s(col).is_empty() && Some(r.s(col).as_str()) != skip_concept).map(|r| { let k = r.s(col); json!({"id": k, "name": dict.by_slug(&k).map(|c| c.name.clone()).unwrap_or(k.clone()), "n": r.i("n")}) }).collect()
    };
    let total: i64 = years.iter().map(|y| y.1).sum();
    json!({
        "total": total,
        "years": years.iter().map(|(y, n)| json!({"year": y, "n": n, "per10k": if *tot.get(y).unwrap_or(&0) > 0 { (*n as f64 * 100_000.0 / *tot.get(y).unwrap_or(&1) as f64).round() / 10.0 } else { 0.0 }})).collect::<Vec<_>>(),
        "recent": now, "before": before,
        "top": top, "newest": newest, "rising": rising,
        "people": people.iter().filter(|r| !r.s("people").is_empty()).map(|r| { let k = r.s("people"); json!({"id": k, "name": names.get(&k).map(|x| x.s("name")).unwrap_or(k.clone()), "n": r.i("n"), "cites": r.i("s")}) }).collect::<Vec<_>>(),
        "techniques": named(&techs, "techniques"),
        "domains": named(&doms, "domains"),
        "categories": cats.iter().filter(|r| !r.s("cats").is_empty()).map(|r| { let k = r.s("cats"); json!({"id": k, "name": concepts::category_name(&k), "n": r.i("n")}) }).collect::<Vec<_>>(),
    })
}

pub async fn topic(c: &Ctx<'_>, id: i64) -> Option<Value> {
    let row = c.soft("the topic", "SELECT id, label, about, terms, field, field_label, papers, recent, growth, cites, first, peak, cats, techniques, domains, x, y FROM ax_topics WHERE id = :t", json!({"t": id})).await.into_iter().next()?;
    let (body, near, field) = tokio::join!(
        scope(c, "topic = :t", json!({"t": id}), None),
        c.soft("the nearest topics (their centroids' similarity)", "SELECT id, label, field_label, papers, growth, score() AS score FROM ax_topics WHERE id != :t ORDER BY similarity(centroid, :v) DESC LIMIT 10", json!({"t": id, "v": {"vector_of": id, "table": "ax_topics", "field": "centroid"}})),
        c.soft("the other topics of its field", "SELECT id, label, papers, growth FROM ax_topics WHERE field = :f AND id != :t ORDER BY papers DESC LIMIT 16", json!({"f": row.i("field"), "t": id})),
    );
    Some(json!({"topic": row, "body": body, "near": near, "field": field}))
}

pub async fn concept(c: &Ctx<'_>, slug: &str) -> Option<Value> {
    let d = concepts::dict().by_slug(slug)?;
    let col = if d.kind == "domain" { "domains" } else { "techniques" };
    let other = if d.kind == "domain" { "techniques" } else { "domains" };
    let cond = format!("{col} = :s");
    let (row, body, topics) = tokio::join!(
        c.soft("the concept", "SELECT * FROM ax_concepts WHERE id = :s", json!({"s": slug})),
        scope(c, &cond, json!({"s": slug}), Some(slug)),
        c.soft("its topics", &format!("SELECT id, label, field_label, papers, growth FROM ax_topics WHERE {col} = :s ORDER BY papers DESC LIMIT 16"), json!({"s": slug})),
    );
    let mut row = row.into_iter().next().unwrap_or_default();
    if let Some(b) = row.get("by_year").and_then(|v| v.as_str()).map(parse_by_year) {
        row.insert("by_year".into(), b);
    }
    Some(json!({"concept": {"id": slug, "kind": d.kind, "name": d.name, "phrases": d.phrases, "acronyms": d.acronyms}, "row": row, "body": body, "topics": topics, "pair": other}))
}

pub async fn category(c: &Ctx<'_>, code: &str) -> Option<Value> {
    let name = concepts::category_name(code);
    let (row, body, topics) = tokio::join!(
        c.soft("the category", "SELECT * FROM ax_cats WHERE id = :c", json!({"c": code})),
        scope(c, "cats = :c", json!({"c": code}), None),
        c.soft("its topics", "SELECT id, label, field_label, papers, growth FROM ax_topics WHERE cats = :c ORDER BY papers DESC LIMIT 20", json!({"c": code})),
    );
    Some(json!({"category": {"id": code, "name": name, "area": concepts::group_of(code)}, "row": row.first(), "body": body, "topics": topics}))
}

pub async fn venue(c: &Ctx<'_>, name: &str) -> Option<Value> {
    let body = scope(c, "venue = :v", json!({"v": name}), None).await;
    Some(json!({"venue": name, "body": body}))
}

/// The atlas: every topic placed on the map, with its field.
pub async fn atlas(c: &Ctx<'_>) -> Value {
    let rows = c.soft("every topic and its place on the map", "SELECT id, label, field, field_label, papers, recent, growth, x, y FROM ax_topics ORDER BY papers DESC LIMIT 5000", Value::Null).await;
    let mut fields: HashMap<i64, (String, i64, f64, f64, i64)> = HashMap::new();
    for r in &rows {
        let e = fields.entry(r.i("field")).or_insert((r.s("field_label"), 0, 0.0, 0.0, 0));
        e.1 += r.i("papers");
        e.2 += r.f("x") * r.i("papers") as f64;
        e.3 += r.f("y") * r.i("papers") as f64;
        e.4 += 1;
    }
    let fields: Vec<Value> = fields.into_iter().map(|(k, (l, n, x, y, t))| json!({"id": k, "label": l, "papers": n, "topics": t, "x": x / n.max(1) as f64, "y": y / n.max(1) as f64})).collect();
    json!({"topics": rows, "fields": fields})
}

/// What a reader should read next, from what they opened and kept here
/// (their events): the recommender's ranking (what came after, what was
/// read together, the nearest to their last papers, popular), fused with
/// PageRank from their papers over the citation graph.
pub async fn for_you(c: &Ctx<'_>, user: &str, area: Option<&str>) -> Value {
    let u = format!("u:{user}");
    let p = json!({"u": u});
    let (mine, list) = tokio::join!(
        c.gsoft("what you opened and kept", "SELECT item, kind, ts FROM ax_events WHERE usr = :u ORDER BY ts DESC LIMIT 30", p.clone()),
        c.gsoft("the recommender's lists for you (what came next, read together, popular)", &format!("SELECT * FROM recommend('{RECOMMENDER}', :u, k => 300)"), p.clone()),
    );
    let mut recent: Vec<String> = Vec::new();
    for r in &mine {
        let i = r.s("item");
        if !recent.contains(&i) {
            recent.push(i);
        }
    }
    let seeds: Vec<String> = recent.iter().take(20).cloned().collect();
    let walked = walk(c, "PageRank from your papers over the citations", "ax_cites", &seeds, 200).await;
    let rec_ids = ids_of(&list, "item");
    let mut pmap = serde_json::Map::new();
    if let Some(a) = area {
        pmap.insert("area".into(), json!(a));
    }
    let rec = fused(c, "for you: the recommender, PageRank from your papers and the nearest to your last one, fused (rrf)", &[rec_ids, walked.clone()], recent.first().map(|s| s.as_str()), &seeds, area.map(|_| "areas = :area"), pmap, 30).await;
    let walked: HashSet<String> = walked.into_iter().collect();
    let seen: HashSet<String> = mine.iter().map(|r| r.s("item")).collect();
    let ids: Vec<String> = mine.iter().map(|r| r.s("item")).collect::<Vec<_>>();
    let mut dedup = Vec::new();
    for i in ids {
        if !dedup.contains(&i) {
            dedup.push(i);
        }
    }
    let history = papers_by_id(c, "your papers", &dedup.into_iter().take(12).collect::<Vec<_>>(), COLS).await;
    let why: HashMap<String, Row> = list.into_iter().map(|r| (r.s("item"), r)).collect();
    let mut rec = rec;
    rec.retain(|r| !seen.contains(&r.s("id")));
    for r in rec.iter_mut() {
        let mut from = Vec::new();
        if let Some(w) = why.get(&r.s("id")) {
            for k in ["next", "co", "popular"] {
                if w.get(k).is_some_and(|v| !v.is_null()) {
                    from.push(k);
                }
            }
        }
        if walked.contains(&r.s("id")) {
            from.push("cites");
        }
        r.insert("why".into(), json!(from));
    }
    json!({"history": history, "papers": rec, "events": mine.len()})
}

/// A reader's event: a paper opened, kept or dismissed. Opened and kept
/// papers teach the recommender (it counts them as they are written).
pub async fn event(c: &Ctx<'_>, user: &str, paper: &str, kind: &str) -> Result<Value, crate::strata::Error> {
    let now = chrono::Utc::now();
    let id = format!("u:{user}|{paper}|{kind}");
    let row = json!({"id": id, "usr": format!("u:{user}"), "item": crate::understand::clean_id(paper), "ts": now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(), "kind": kind});
    let t0 = std::time::Instant::now();
    let r = c.g.upsert("ax_events", vec![row.clone()]).await;
    c.trace.push("your event (the recommender counts it as it is written)", "POST /v1/write {table: ax_events}", &row, 1, t0.elapsed().as_secs_f64() * 1000.0, r.as_ref().err().map(|e| e.message.as_str()));
    if kind == "save" {
        let lib = json!({"id": format!("{user}|{paper}"), "usr": user, "paper": crate::understand::clean_id(paper), "at": now.format("%Y-%m-%dT%H:%M:%SZ").to_string()});
        let _ = c.g.upsert("ax_library", vec![lib]).await;
    }
    r.map(|_| json!({"ok": true}))
}

pub async fn library(c: &Ctx<'_>, user: &str) -> Value {
    let rows = c.gsoft("your library", "SELECT paper, at FROM ax_library WHERE usr = :u ORDER BY at DESC LIMIT 200", json!({"u": user})).await;
    let ids = ids_of(&rows, "paper");
    let papers = papers_by_id(c, "the papers in it", &ids, COLS).await;
    json!({"papers": papers})
}
