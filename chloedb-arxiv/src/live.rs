//! The live feed: new and revised papers from arXiv's OAI-PMH interface
//! (`arXivRaw`, the day's records a page of up to 1,300 at a time, three
//! seconds between pages), made into rows as `prepare` makes them and
//! written as they come. A paper already here keeps its citations and its
//! topic; a new one is placed in the topic whose centroid its text is
//! nearest (ChloeDB embeds the text where the query runs), and its people
//! who are new get a row. Where it got to is kept in `ax_state`.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::paper;
use crate::raw::{self, Raw};
use crate::strata::{RowExt, Strata};

pub const OAI: &str = "https://oaipmh.arxiv.org/oai";

#[derive(Default)]
pub struct Live {
    pub state: Mutex<Value>,
}

impl Live {
    pub fn status(&self) -> Value {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn set(&self, k: &str, v: Value) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.is_object() {
            *s = json!({});
        }
        s[k] = v;
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// How much a topic's mix of categories counts against nearness when a
/// paper is placed: `tools/topics.py --prior`, so a new paper lands where
/// the clustering would have put it. Nearness alone let topics of one
/// field gather papers of others that only wrote alike.
pub const PRIOR: f64 = 0.01;

/// The topic for a new paper among the nearest by meaning (`s`, the
/// similarity): nearness plus PRIOR times the log share of the paper's
/// primary category among the topic's papers (`shares`, a JSON object of
/// shares, `papers` its count), smoothed as the clustering smooths it.
pub fn place(near: &[crate::strata::Row], pcat: &str) -> Option<i64> {
    near.iter().map(|t| {
        let n = t.i("papers").max(0) as f64;
        let share = serde_json::from_str::<Value>(&t.s("shares")).ok().and_then(|v| v.get(pcat).and_then(|x| x.as_f64())).unwrap_or(0.0);
        (t.i("id"), t.f("s") + PRIOR * ((share * n + 0.5) / (n + 50.0)).ln())
    }).max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|x| x.0)
}

/// Writes the rows of `records`, keeping what is known of papers already
/// here. Returns how many were new.
pub async fn write(db: &Strata, graph: &Strata, records: &[Raw]) -> Result<(usize, usize), String> {
    if records.is_empty() {
        return Ok((0, 0));
    }
    let ids: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
    let known = db.query("SELECT id, cites, refs, cited_in, topic FROM ax_papers WHERE id IN (:ids)", json!({"ids": ids})).await.map_err(|e| e.to_string())?;
    let known: std::collections::HashMap<String, serde_json::Map<String, Value>> = known.into_iter().map(|r| (r.s("id"), r)).collect();
    let mut rows = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();
    let mut people: Vec<(String, String, String, String)> = Vec::new();
    let mut new = 0;
    for r in records {
        let k = known.get(&r.id);
        let cites = k.map(|k| k.i("cites") as u32).unwrap_or(0);
        let topic = k.and_then(|k| k.get("topic").and_then(|t| t.as_i64()));
        let topic = match topic {
            Some(t) => Some(t),
            None => {
                let text = format!("{}\n{}", crate::concepts::one_line(&r.title), crate::concepts::one_line(&r.abstract_));
                let near = db.query("SELECT id, papers, shares, score() AS s FROM ax_topics ORDER BY similarity(centroid, :v) DESC LIMIT 10", json!({"v": {"embed": text, "field": "emb", "table": "ax_papers"}})).await.unwrap_or_default();
                place(&near, r.cats.first().map(|c| c.as_str()).unwrap_or(""))
            }
        };
        let b = paper::build(r, cites, topic);
        let mut row = b.row;
        row["refs"] = json!(k.map(|k| k.i("refs")).unwrap_or(0));
        row["cited_in"] = json!(k.map(|k| k.i("cited_in")).unwrap_or(0));
        if k.is_none() {
            new += 1;
            nodes.push(json!({"id": r.id}));
            for p in &b.people {
                people.push((p.key.clone(), p.name.clone(), row["created"].as_str().unwrap_or("").to_string(), b.primary.clone()));
            }
        }
        rows.push(row);
    }
    // New people: a row each, unless they have one.
    if !people.is_empty() {
        let keys: Vec<String> = people.iter().map(|p| p.0.clone()).collect();
        let have: std::collections::HashSet<String> = db.query("SELECT id FROM ax_authors WHERE id IN (:k)", json!({"k": keys})).await.map_err(|e| e.to_string())?.into_iter().map(|r| r.s("id")).collect();
        let mut add = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (k, name, at, cat) in people {
            if have.contains(&k) || !seen.insert(k.clone()) {
                continue;
            }
            nodes.push(json!({"id": k}));
            add.push(json!({"id": k, "name": name, "papers": 1, "first_author": 0, "cites": 0, "h": 0, "first": at, "last": at, "recent": 1, "cats": [cat], "techniques": [], "coauthors": 0}));
        }
        db.upsert("ax_authors", add).await.map_err(|e| e.to_string())?;
    }
    let n = rows.len();
    db.upsert("ax_papers", rows).await.map_err(|e| e.to_string())?;
    // The graph's nodes, for walks from the new papers and people.
    if !nodes.is_empty() {
        graph.upsert("ax_nodes", nodes).await.map_err(|e| e.to_string())?;
    }
    Ok((n, new))
}

/// One round: every record since the day kept in `ax_state` (or `from`),
/// page by page, then the day moved on.
pub async fn round(db: &Strata, graph: &Strata, live: &Live, from: Option<&str>) -> Result<usize, String> {
    let http = reqwest::Client::builder().user_agent("chloedb-arxiv/0.3 (OAI-PMH harvester; 3 s between requests)").timeout(Duration::from_secs(180)).build().map_err(|e| e.to_string())?;
    let kept = graph.query("SELECT val FROM ax_state WHERE id = 'oai'", Value::Null).await.ok().and_then(|r| r.first().map(|r| r.s("val")));
    // Two days before the last round's: a record can be dated the day before
    // it shows (arXiv dates an announcement by its day in New York, and a
    // round can run before that day's records are out), and writing a
    // paper again changes nothing.
    let back = |d: &str| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(|d| (d - chrono::Duration::days(2)).format("%Y-%m-%d").to_string());
    let since = from.map(str::to_string).or(kept.as_deref().and_then(back)).unwrap_or_else(|| (chrono::Utc::now() - chrono::Duration::days(3)).format("%Y-%m-%d").to_string());
    live.set("since", json!(since));
    live.set("running", json!(true));
    let mut url = format!("{OAI}?verb=ListRecords&metadataPrefix=arXivRaw&from={since}");
    let mut total = 0;
    let mut news = 0;
    let started = chrono::Utc::now().format("%Y-%m-%d").to_string();
    loop {
        let mut body = String::new();
        for attempt in 0..6u64 {
            match http.get(&url).send().await {
                Ok(r) if r.status().as_u16() == 503 => {
                    let wait = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(30u64);
                    tokio::time::sleep(Duration::from_secs(wait.min(300))).await;
                }
                Ok(r) if r.status().is_success() => {
                    body = r.text().await.unwrap_or_default();
                    break;
                }
                Ok(r) => {
                    let s = r.status();
                    if attempt == 5 {
                        live.set("error", json!(format!("OAI-PMH answered {s}")));
                    }
                    tokio::time::sleep(Duration::from_secs(10 * (attempt + 1))).await;
                }
                Err(e) => {
                    if attempt == 5 {
                        live.set("error", json!(e.to_string()));
                    }
                    tokio::time::sleep(Duration::from_secs(10 * (attempt + 1))).await;
                }
            }
        }
        if body.is_empty() {
            live.set("running", json!(false));
            return Err("OAI-PMH did not answer".into());
        }
        if body.contains("noRecordsMatch") {
            break;
        }
        let (records, token) = raw::oai_page(&body);
        let (n, new) = write(db, graph, &records).await?;
        total += n;
        news += new;
        live.set("written", json!(total));
        live.set("new", json!(news));
        live.set("at", json!(chrono::Utc::now().to_rfc3339()));
        if token.is_empty() {
            break;
        }
        url = format!("{OAI}?verb=ListRecords&resumptionToken={}", urlencode(&token));
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    // The next round starts at today: records are dated by the day they changed.
    let _ = graph.upsert("ax_state", vec![json!({"id": "oai", "val": started, "n": total, "at": chrono::Utc::now().to_rfc3339()})]).await;
    live.set("running", json!(false));
    live.set("last_round", json!({"records": total, "new": news, "since": since, "at": chrono::Utc::now().to_rfc3339()}));
    Ok(total)
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

/// Rounds for ever, `every` apart (arXiv announces once a day, so hours
/// apart is plenty).
pub async fn run(db: Strata, graph: Strata, live: std::sync::Arc<Live>, every: Duration) {
    loop {
        if let Err(e) = round(&db, &graph, &live, None).await {
            live.set("error", json!(e));
            tracing::warn!("live feed: {e}");
        }
        tokio::time::sleep(every).await;
    }
}
