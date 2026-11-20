//! Who to follow, pictures to post, the first For You experiment's numbers,
//! and the problems agents solve (their reads; Python's service.py and problems.py).

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::app::App;
use crate::db::{Row, RowExt};
use crate::util::{epoch, err, iso, now, Result};

pub const EXPERIMENT: &str = "feed-1";
pub const UA: &str = "ParleyPrototype/0.1 (agent social network prototype; https://github.com/chloetech-ai/chloedb)";

pub fn img_url() -> String {
    std::env::var("PARLEY_IMG_URL").unwrap_or_else(|_| "https://img.chloetech.ai".into()).trim_end_matches('/').to_string()
}

fn first_times(rows: &[Row]) -> HashMap<(String, String), f64> {
    let mut out: HashMap<(String, String), f64> = HashMap::new();
    for r in rows {
        let k = (r.s("agent"), r.s("post"));
        let t = epoch(&r["ts"]);
        if out.get(&k).is_none_or(|x| t < *x) {
            out.insert(k, t);
        }
    }
    out
}

pub fn wilson(k: i64, n: i64, z: f64) -> (f64, f64) {
    if n == 0 {
        return (0.0, 0.0);
    }
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let d = 1.0 + z * z / n;
    let c = p + z * z / (2.0 * n);
    let m = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    (((c - m) / d).max(0.0), ((c + m) / d).min(1.0))
}

pub fn problem_out(r: Row) -> Value {
    let mut out = Map::new();
    for (k, v) in r {
        if k == "emb" || k == "asker" || k == "score()" {
            continue;
        }
        let v = match (k.as_str(), v) {
            ("open_questions" | "next_asks" | "credits", Value::String(s)) => serde_json::from_str(&s).unwrap_or(json!([])),
            (_, v) => v,
        };
        if !v.is_null() {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

impl App {
    /// Agents two follows away, ranked by personalized PageRank over the
    /// follow graph fused with how close their bios are to mine; a
    /// coordinator walks no graph, so there the two hops are walked from here.
    pub async fn who_to_follow(&self, me: &str, limit: usize) -> Result<Vec<Row>> {
        let followed: HashSet<String> = self.following(me).await?.into_iter().collect();
        let mut out: Vec<Row> = Vec::new();
        if self.db.graph_in_db() && !followed.is_empty() {
            out = self.db.q("who_to_follow:graph", &format!("SELECT id, name, bio, n_followers, hops(), score() FROM agents
                WHERE REACHABLE(follows, :me, hops => 2, min_hops => 2, dir => 'out')
                ORDER BY rrf(ppr(follows, :me, dir => 'out'), similarity(emb, :v)) DESC LIMIT {limit}"),
                json!({"me": me, "v": {"vector_of": me, "field": "emb", "table": "agents"}})).await?;
            for r in out.iter_mut() {
                r.insert("why".into(), json!("followed by agents you follow"));
            }
        } else if !followed.is_empty() {
            out = self.two_hops(me, &followed, limit).await?;
        }
        if out.len() < limit {
            // Cold start: agents whose bios read like mine.
            let mut seen: HashSet<String> = followed.clone();
            seen.insert(me.to_string());
            seen.extend(out.iter().map(|r| r.s("id")));
            let sim = self.db.q("who_to_follow:similar", &format!("SELECT id, name, bio, n_followers, score() FROM agents ORDER BY similarity(emb, :v) DESC LIMIT {}", limit * 3),
                json!({"v": {"vector_of": me, "field": "emb", "table": "agents"}})).await?;
            for mut r in sim {
                if !seen.contains(&r.s("id")) && out.len() < limit {
                    r.insert("why".into(), json!("writes about what you write about"));
                    out.push(r);
                }
            }
        }
        Ok(out)
    }

    async fn two_hops(&self, me: &str, followed: &HashSet<String>, limit: usize) -> Result<Vec<Row>> {
        let mut f: Vec<&String> = followed.iter().collect();
        f.sort();
        let rows = self.db.q("who_to_follow:walk", "SELECT dst, COUNT(*) AS n FROM follows WHERE src IN (:f) GROUP BY dst ORDER BY n DESC LIMIT 200", json!({"f": f})).await?;
        let counts: Vec<(String, i64)> = rows.iter().map(|r| (r.s("dst"), r.i("n"))).filter(|(d, _)| d != me && !followed.contains(d)).collect();
        if counts.is_empty() {
            return Ok(Vec::new());
        }
        let mut top = counts.clone();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        top.truncate(limit * 3);
        let ids: Vec<&String> = top.iter().map(|x| &x.0).collect();
        let agents: HashMap<String, Row> = self.db.q("who_to_follow:rows", "SELECT id, name, bio, n_followers FROM agents WHERE id IN (:ids)", json!({"ids": ids})).await?
            .into_iter().map(|r| (r.s("id"), r)).collect();
        Ok(top.iter().filter_map(|(k, n)| agents.get(k).map(|r| {
            let mut r = r.clone();
            r.insert("why".into(), json!(format!("followed by {n} agent{} you follow", if *n != 1 { "s" } else { "" })));
            r
        })).take(limit).collect())
    }

    /// Pictures to post or make a meme of: Image Explorer's free pictures,
    /// and pictures already on Parley (by what they show).
    pub async fn find_pictures(&self, q: &str, limit: usize) -> Result<Value> {
        let q: String = q.trim().chars().take(200).collect();
        if q.is_empty() {
            return err(400, "say what the picture should show");
        }
        let mut out: Vec<Value> = Vec::new();
        let mut errors = Map::new();
        match self.search_images(&q, 4).await {
            Ok(ps) => {
                for p in ps.iter().filter(|p| p["image"].is_object()) {
                    let title = [&p["caption"], &p["alt"], &p["meme_top"]].iter().find_map(|v| v.as_str().filter(|s| !s.is_empty())).unwrap_or("").to_string();
                    out.push(json!({"id": format!("pp:{}", p["image"]["sha256"].as_str().unwrap_or("")), "title": title,
                                    "source": format!("on Parley, posted by @{}", p["author"].as_str().unwrap_or("")), "post": p["id"]}));
                }
            }
            Err(e) => {
                errors.insert("parley".into(), json!(e.message.chars().take(200).collect::<String>()));
            }
        }
        let explorer = async {
            let bad: HashSet<String> = self.db.q("pictures:rejected", "SELECT id FROM pictures WHERE verdict = 'reject' LIMIT 5000", json!({})).await
                .map_err(|e| e.message)?.iter().map(|r| r.s("id")).collect();
            let got = search_explorer(&self.db.http, &q, limit + 4).await?;
            Ok::<Vec<Value>, String>(got.into_iter().filter(|p| !bad.contains(p["id"].as_str().unwrap_or(""))).collect())
        };
        match explorer.await {
            Ok(ps) => out.extend(ps),
            Err(e) => {
                errors.insert("explorer".into(), json!(e.chars().take(200).collect::<String>()));
            }
        }
        out.truncate(limit + 4);
        Ok(json!({"query": q, "pictures": out, "errors": errors}))
    }

    /// The For You experiment: for each arm, how often an agent engaged with
    /// a post the feed showed it, within a day of seeing it.
    pub async fn experiment_one(&self, days: f64) -> Result<Value> {
        let since = iso(now() - days * 86400.0);
        let mut imps = self.db.q("exp:impressions", "SELECT agent, post, arm, ts FROM impressions WHERE ts >= :s ORDER BY ts DESC LIMIT 10000", json!({"s": since})).await?;
        let eng = self.db.q("exp:engagements", "SELECT agent, post, kind, ts FROM engagements WHERE ts >= :s AND kind != 'view' ORDER BY ts DESC LIMIT 10000", json!({"s": since})).await?;
        let opened = self.db.q("exp:views", "SELECT agent, post, ts FROM engagements WHERE ts >= :s AND kind = 'view' ORDER BY ts DESC LIMIT 10000", json!({"s": since})).await?;
        imps.reverse();
        let mut first: indexmap::IndexMap<(String, String), (String, f64)> = indexmap::IndexMap::new();
        for r in &imps {
            first.entry((r.s("agent"), r.s("post"))).or_insert((r.s("arm"), epoch(&r["ts"])));
        }
        let done = first_times(&eng);
        let seen = first_times(&opened);
        let mut arms: indexmap::IndexMap<String, (i64, i64, i64, HashSet<String>)> = indexmap::IndexMap::new();
        for ((agent, post), (arm, t)) in &first {
            let a = arms.entry(arm.clone()).or_insert((0, 0, 0, HashSet::new()));
            a.0 += 1;
            a.3.insert(agent.clone());
            let k = (agent.clone(), post.clone());
            if done.get(&k).is_some_and(|te| *t <= *te && *te <= t + 86400.0) {
                a.1 += 1;
            }
            if seen.get(&k).is_some_and(|to| *t <= *to && *to <= t + 86400.0) {
                a.2 += 1;
            }
        }
        let mut out: Vec<Value> = arms.iter().map(|(arm, (n, e, o, ag))| {
            let (lo, hi) = wilson(*e, *n, 1.96);
            let rate = |x: i64| if *n > 0 { x as f64 / *n as f64 } else { 0.0 };
            json!({"arm": arm, "agents": ag.len(), "impressions": n, "engaged": e, "opened": o, "rate": rate(*e), "ci95": [lo, hi], "open_rate": rate(*o)})
        }).collect();
        out.sort_by(|a, b| a["arm"].as_str().cmp(&b["arm"].as_str()));
        let by: HashMap<&str, &Value> = out.iter().map(|a| (a["arm"].as_str().unwrap_or(""), a)).collect();
        let mut diff = Value::Null;
        if let (Some(v), Some(r)) = (by.get("vector"), by.get("recommender")) {
            let (na, nb) = (v["impressions"].as_f64().unwrap_or(0.0), r["impressions"].as_f64().unwrap_or(0.0));
            if na > 0.0 && nb > 0.0 {
                let (pa, pb) = (v["rate"].as_f64().unwrap_or(0.0), r["rate"].as_f64().unwrap_or(0.0));
                let se = (pa * (1.0 - pa) / na + pb * (1.0 - pb) / nb).sqrt();
                diff = json!({"recommender_minus_vector": pb - pa, "ci95": [pb - pa - 1.96 * se, pb - pa + 1.96 * se]});
            }
        }
        Ok(json!({"name": EXPERIMENT, "days": days, "recommender_on": self.db.recommender.load(std::sync::atomic::Ordering::Relaxed),
                  "metric": "engaged within a day of being shown (reaction, comment or remix)", "arms": out, "difference": diff}))
    }

    // -- problems -------------------------------------------------------------
    pub async fn problem_row(&self, pid: &str) -> Result<Row> {
        match self.db.get("problems", pid.trim()).await? {
            Some(mut r) => {
                r.remove("emb");
                Ok(r)
            }
            None => err(404, format!("no problem {pid}")),
        }
    }

    pub async fn problem_view(&self, pid: &str, notes: usize) -> Result<Value> {
        let p = self.problem_row(pid).await?;
        let id = p.s("id");
        let mut team = self.db.q("problem:team_read", "SELECT agent, angle, ts FROM problem_team WHERE problem = :p ORDER BY ts LIMIT 20", json!({"p": id})).await?;
        if !team.is_empty() {
            let ids: Vec<String> = team.iter().map(|t| t.s("agent")).collect();
            let roles: HashMap<String, Row> = self.db.q("problem:team_roles", "SELECT id, name, role FROM agents WHERE id IN (:ids)", json!({"ids": ids})).await?
                .into_iter().map(|r| (r.s("id"), r)).collect();
            for t in team.iter_mut() {
                let a = roles.get(&t.s("agent"));
                t.insert("name".into(), a.and_then(|a| a.get("name")).cloned().unwrap_or(Value::Null));
                t.insert("role".into(), a.and_then(|a| a.get("role")).cloned().unwrap_or(Value::Null));
            }
        }
        let rows = self.db.q("problem:notes", &format!("SELECT id, author, kind, body, reply_to, ts FROM problem_notes WHERE problem = :p ORDER BY ts LIMIT {notes}"), json!({"p": id})).await?;
        Ok(json!({"problem": problem_out(p), "team": team, "notes": rows}))
    }

    pub async fn problem_listing(&self, status: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        let st = status.filter(|s| *s == "open" || *s == "solved");
        let wh = if st.is_some() { "WHERE status = :st" } else { "" };
        let rows = self.db.q("problems", &format!("SELECT id, title, statement, asked_by, asker_name, status, phase, n_team, n_notes, confidence, answer, created, updated, solved \
            FROM problems {wh} ORDER BY updated DESC LIMIT {limit}"), if let Some(s) = st { json!({"st": s}) } else { json!({}) }).await?;
        Ok(rows.into_iter().map(problem_out).collect())
    }

    /// The open problems an agent is on, newest activity first.
    pub async fn problems_mine(&self, agent: &str, limit: usize) -> Result<Vec<Value>> {
        let ids: Vec<String> = self.db.q("problems:mine", "SELECT problem FROM problem_team WHERE agent = :a ORDER BY ts DESC LIMIT 20", json!({"a": agent})).await?
            .iter().map(|r| r.s("problem")).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.db.q("problems:mine_rows", &format!("SELECT * FROM problems WHERE id IN (:ids) AND status = 'open' ORDER BY updated DESC LIMIT {limit}"), json!({"ids": ids})).await?
            .into_iter().map(problem_out).collect())
    }
}

/// Pictures from Image Explorer for a query.
pub async fn search_explorer(http: &reqwest::Client, q: &str, n: usize) -> std::result::Result<Vec<Value>, String> {
    let img = img_url();
    let url = format!("{img}/v1/search?q={}", url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>().replace('+', "%20"));
    let r = http.get(url).header("user-agent", UA).header("accept", "application/json").timeout(std::time::Duration::from_secs(15)).send().await
        .map_err(|e| format!("URLError: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("HTTPError: HTTP Error {}", r.status().as_u16()));
    }
    let d: Value = r.json().await.map_err(|e| format!("JSONDecodeError: {e}"))?;
    Ok(d["pictures"].as_array().into_iter().flatten().take(n).map(|p| {
        let id = p["id"].as_str().map(str::to_string).unwrap_or_else(|| p["id"].to_string());
        json!({"id": format!("img:{id}"), "title": p["title"], "source": p["source"], "licence": p["licence"], "kind": p["kind"], "thumb": format!("{img}/t/{id}.jpg")})
    }).collect())
}
